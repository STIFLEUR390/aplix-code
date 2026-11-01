#![forbid(unsafe_code)]
//! The `apc` binary: composition root for every layer.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use directories::ProjectDirs;

use apc_core::chat::{ChatEvent, Message, Role};
use apc_core::config::{self, Layer, Loaded, Source};
use apc_memory::SqliteStore;
use apc_traits::{Approver, DecisionProvider, MemoryStore};

mod serve;
mod telemetry;
mod tui;

/// `apc --version` prints the CLI version and the decision model (SPEC.md §2.1).
const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "\ndecision model: none (heuristic fallback)"
);

/// `APC_*` variables that override configuration (the env layer).
/// `APC_API_KEY` is deliberately absent: keys are read by `apc-models`, never stored in the
/// configuration (SPEC.md §3.2).
const ENV_KEYS: &[(&str, &str)] = &[
    ("APC_PROVIDER", "provider.name"),
    ("APC_MODEL", "provider.model"),
    ("APC_BASE_URL", "provider.base_url"),
    ("APC_PERMISSION_THRESHOLD", "decision.threshold"),
    ("APC_DECISION_ENDPOINT", "decision.endpoint"),
    ("APC_TELEMETRY", "telemetry"),
    ("APC_SEARXNG_URL", "tools.searxng_url"),
];

#[derive(Parser)]
#[command(
    name = "apc",
    version = VERSION,
    long_version = VERSION,
    about = "Aplix Code — terminal coding agent",
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a commented apc.toml in the current directory.
    Init(InitArgs),
    /// Show the effective configuration and the layer behind each value.
    Config(Overrides),
    /// Send one prompt and stream the answer.
    Run(RunArgs),
    /// Interactive session; one message per line.
    Chat(ChatArgs),
    /// Manage the local decision models.
    Model(ModelArgs),
    /// Serve the agent on 127.0.0.1 for a UI client.
    Serve(ServeArgs),
}

#[derive(Args)]
struct InitArgs {
    /// Overwrite an existing apc.toml.
    #[arg(long)]
    force: bool,
}

/// Configuration overrides shared by every command (the flag layer).
#[derive(Args)]
struct Overrides {
    /// Override the provider.
    #[arg(long)]
    provider: Option<String>,
    /// Override the model.
    #[arg(long)]
    model: Option<String>,
    /// Override the destructive-command threshold.
    #[arg(long, value_name = "P")]
    permission_threshold: Option<f64>,
}

#[derive(Args)]
struct RunArgs {
    /// Prompt sent as the first user message.
    #[arg(required = true, num_args = 1.., value_name = "PROMPT")]
    prompt: Vec<String>,
    #[command(flatten)]
    overrides: Overrides,
}

#[derive(Args)]
struct ChatArgs {
    /// Resume the most recent session.
    #[arg(long)]
    r#continue: bool,
    #[command(flatten)]
    overrides: Overrides,
}

#[derive(Args)]
struct ServeArgs {
    /// Port on loopback.
    #[arg(long, default_value_t = 7717)]
    port: u16,
    #[command(flatten)]
    overrides: Overrides,
}

#[derive(Args)]
struct ModelArgs {
    #[command(subcommand)]
    command: ModelCommand,
}

#[derive(Subcommand)]
enum ModelCommand {
    /// List the published models and what is on disk.
    List,
    /// Download a model, resuming an interrupted one.
    Download {
        /// Model id (`apc model list`).
        id: String,
    },
    /// Show whether a model is installed.
    Status {
        /// Model id.
        id: String,
    },
    /// Re-check an installed model's checksum.
    Verify {
        /// Model id.
        id: String,
    },
    /// Delete a model and any partial download.
    Remove {
        /// Model id.
        id: String,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    #[cfg(debug_assertions)]
    load_dotenv(); // dev only: a release build never reads a stray .env
    let cli = Cli::parse();
    // Held until the end of main so a queued event flushes on drop; a `()` guard without the
    // `telemetry` feature is the point of the binding, hence the allow.
    #[allow(clippy::let_unit_value)]
    let _sentry = telemetry::init(telemetry_enabled(&cli));

    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            telemetry::capture(&error);
            eprintln!("error: {error:?}");
            ExitCode::FAILURE
        }
    }
}

/// Dev convenience: read `.env` (`KEY=VALUE`, `#` comments) so debug runs pick up the local DSN
/// and key. Existing variables win.
#[cfg(debug_assertions)]
fn load_dotenv() {
    let Ok(text) = std::fs::read_to_string(".env") else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if std::env::var_os(key.trim()).is_none() {
            // No quote stripping: the shipped .env has no quoted values.
            std::env::set_var(key.trim(), value.trim());
        }
    }
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Init(args) => init(&args),
        Command::Config(args) => show_config(&args),
        Command::Run(args) => run_prompt(&args).await,
        Command::Chat(args) => chat(&args).await,
        Command::Model(args) => model(&args).await,
        Command::Serve(args) => {
            let (loaded, _) = resolve(&args.overrides)?;
            serve::serve(loaded.config, args.port).await
        }
    }
}

fn overrides(command: &Command) -> Option<&Overrides> {
    match command {
        Command::Init(_) => None,
        Command::Config(args) => Some(args),
        Command::Run(args) => Some(&args.overrides),
        Command::Chat(args) => Some(&args.overrides),
        Command::Model(_) => None,
        Command::Serve(args) => Some(&args.overrides),
    }
}

/// Telemetry needs the config flag, and `apc init` is about to write that very config.
fn telemetry_enabled(cli: &Cli) -> bool {
    match overrides(&cli.command) {
        Some(args) => resolve(args).map_or_else(
            |_| telemetry::wants_from_env(),
            |(loaded, _)| loaded.config.telemetry,
        ),
        None => telemetry::wants_from_env(),
    }
}

async fn run_prompt(args: &RunArgs) -> Result<()> {
    let (loaded, _) = resolve(&args.overrides)?;
    let agent = agent(&loaded.config, std::sync::Arc::new(StdinApprover))?;
    let prompt = args.prompt.join(" ");
    let mut messages = Vec::new();
    apply_skill(&mut messages, &current_dir()?, &prompt);
    messages.push(Message::user(prompt));
    agent.run(&mut messages, print_delta).await?;
    println!();
    Ok(())
}

async fn chat(args: &ChatArgs) -> Result<()> {
    let (loaded, _) = resolve(&args.overrides)?;
    let store = SqliteStore::new()?;
    let session = opened_session(&store, args.r#continue).await?;
    let messages = store.load(Some(&session)).await?;

    if !tui::available() {
        return repl(loaded.config, store, session, messages).await;
    }

    let (approver, asks) = tui::TuiApprover::channel();
    let agent = agent(&loaded.config, std::sync::Arc::new(approver))?;
    tui::chat(
        std::sync::Arc::new(agent),
        session,
        messages,
        asks,
        current_dir()?,
    )
    .await
}

/// Resumes the latest session or starts a new one.
async fn opened_session(store: &SqliteStore, resume: bool) -> Result<apc_core::chat::SessionId> {
    if !resume {
        return Ok(store.new_session());
    }
    let Some(session) = store.latest_session()? else {
        return Ok(store.new_session());
    };
    let messages = store.load(Some(&session)).await?;
    eprintln!(
        "continuing session {} ({} messages)",
        session,
        messages.len()
    );
    Ok(session)
}

/// The plain stdin/stdout REPL, used whenever the view has no terminal to draw on.
async fn repl(
    config: config::Config,
    store: SqliteStore,
    session: apc_core::chat::SessionId,
    mut messages: Vec<Message>,
) -> Result<()> {
    let agent = agent(&config, std::sync::Arc::new(StdinApprover))?;
    let root = current_dir()?;

    loop {
        print!("› ");
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            break;
        }
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        if matches!(text, "exit" | "quit") {
            break;
        }
        let question = Message::user(text);
        apply_skill(&mut messages, &root, text);
        apply_memory(&store, &mut messages, &session, text).await;
        store.append(&session, &question).await?;
        messages.push(question);

        // The agent appends the assistant turn and every tool exchange; persist what it added.
        let before = messages.len();
        agent.run(&mut messages, print_delta).await?;
        println!();
        for message in &messages[before..] {
            store.append(&session, message).await?;
        }
    }
    Ok(())
}

/// Print streamed text as it arrives.
fn print_delta(event: &ChatEvent) {
    if let ChatEvent::Delta(delta) = event {
        print!("{delta}");
        let _ = io::stdout().flush();
    }
}

/// Put the skill matching `prompt` at the head of the conversation.
///
/// Skills are Markdown files at `<root>/skills/<name>.md` (SPEC.md §7). A missing file means no
/// skill, never an error. The message is not persisted: it is re-derived from each prompt.
pub(crate) fn apply_skill(messages: &mut Vec<Message>, root: &std::path::Path, prompt: &str) {
    let Some(name) = apc_decision::classify(prompt).skill() else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(root.join("skills").join(format!("{name}.md"))) else {
        return;
    };
    messages.retain(|message| message.role != Role::System);
    messages.insert(0, Message::system(text.trim()));
}

/// Puts what earlier sessions said about `prompt` in front of it, best match first.
///
/// The session being written is left out: its messages are already in the conversation. A store
/// that cannot answer is not an error here — the turn goes on without recall.
pub(crate) async fn apply_memory(
    store: &SqliteStore,
    messages: &mut Vec<Message>,
    session: &apc_core::chat::SessionId,
    prompt: &str,
) {
    const LIMIT: usize = 3;
    let Ok(recalls) = store.search(prompt, LIMIT).await else {
        return;
    };
    let recalls: Vec<_> = recalls
        .into_iter()
        .filter(|recall| &recall.session != session)
        .collect();
    if recalls.is_empty() {
        return;
    }
    let mut text = String::from("Earlier sessions said (context only, may be irrelevant):\n");
    for recall in &recalls {
        text.push_str(&format!("- {}: {}\n", recall.session, recall.snippet));
    }
    match messages.first_mut() {
        Some(first) if first.role == Role::System => {
            first.content = format!("{text}\n{}", first.content);
        }
        _ => messages.insert(0, Message::system(text)),
    }
}

/// One agent with the built-in tools and the heuristic guard.
/// The heuristic guard always answers; the `open-jev` build asks the model first when an endpoint
/// is configured, and the cascade keeps a dead sidecar from blocking the CLI.
fn decision_provider(_endpoint: Option<&str>) -> std::sync::Arc<dyn DecisionProvider> {
    #[cfg(feature = "open-jev")]
    if let Some(endpoint) = _endpoint {
        return std::sync::Arc::new(apc_decision::Cascade::new(
            std::sync::Arc::new(apc_decision::JevProvider::new(endpoint)),
            std::sync::Arc::new(apc_decision::HeuristicProvider),
        ));
    }
    std::sync::Arc::new(apc_decision::HeuristicProvider)
}

pub(crate) fn agent(
    config: &config::Config,
    approver: std::sync::Arc<dyn Approver>,
) -> Result<apc_agent::Agent> {
    let adapter: std::sync::Arc<dyn apc_traits::ModelAdapter> =
        std::sync::Arc::from(apc_models::build(&config.provider)?);
    let decision = decision_provider(config.decision.endpoint.as_deref());
    let tools = apc_plugins::default_tools(std::env::current_dir()?, decision, approver, config);
    Ok(apc_agent::Agent::new(adapter, tools))
}

/// Asks the terminal before a destructive command runs.
struct StdinApprover;

impl Approver for StdinApprover {
    fn confirm(&self, command: &str, risk: f64) -> apc_core::Result<bool> {
        eprint!(
            "\nabout to run — {:.0}% destructive risk\n  {command}\nrun it? [y/N] ",
            risk * 100.0
        );
        io::stderr().flush().ok();
        let mut line = String::new();
        let read = io::stdin().read_line(&mut line).map_err(|error| {
            apc_core::Error::Tool(format!("cannot read the confirmation: {error}"))
        })?;
        Ok(read > 0 && matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
    }
}

async fn model(args: &ModelArgs) -> Result<()> {
    let store = apc_decision::Store::new()?;
    match &args.command {
        ModelCommand::List => {
            if apc_decision::CATALOG.is_empty() {
                println!(
                    "no decision model is published yet: the heuristic guard needs none (SPEC.md §8.11)"
                );
                return Ok(());
            }
            for entry in apc_decision::CATALOG {
                let state = match store.status(entry.id)? {
                    apc_decision::Status::Installed => "installed".to_owned(),
                    apc_decision::Status::Partial(bytes) => format!("partial ({bytes} bytes)"),
                    apc_decision::Status::Missing => "missing".to_owned(),
                };
                println!(
                    "{:<16} {:>7} MB  {state}",
                    entry.id,
                    entry.bytes / 1_000_000
                );
            }
        }
        ModelCommand::Download { id } => {
            let entry = apc_decision::find(id)?;
            match store.status(id)? {
                apc_decision::Status::Installed => {
                    println!(
                        "{id} is already installed at {}",
                        store.installed(id).display()
                    );
                    return Ok(());
                }
                apc_decision::Status::Partial(bytes) => {
                    eprintln!("resuming {id} at {bytes} bytes");
                }
                apc_decision::Status::Missing => {}
            }
            let mut shown = 0u64;
            let path = store
                .download(entry, |written| {
                    let percent = written * 100 / entry.bytes.max(1);
                    if percent >= shown + 5 || written == entry.bytes {
                        shown = percent;
                        eprint!("\r{id}: {percent}%");
                    }
                })
                .await?;
            eprintln!();
            println!("installed {} at {}", entry.id, path.display());
        }
        ModelCommand::Status { id } => {
            apc_decision::find(id)?;
            match store.status(id)? {
                apc_decision::Status::Installed => {
                    println!("{id}: installed at {}", store.installed(id).display());
                }
                apc_decision::Status::Partial(bytes) => println!("{id}: partial, {bytes} bytes"),
                apc_decision::Status::Missing => println!("{id}: missing"),
            }
        }
        ModelCommand::Verify { id } => {
            let entry = apc_decision::find(id)?;
            if store.verify(entry)? {
                println!("{id}: checksum matches");
            } else {
                bail!("{id}: checksum mismatch, remove it and download again");
            }
        }
        ModelCommand::Remove { id } => {
            if store.remove(id)? {
                println!("removed {id}");
            } else {
                println!("{id} was not present");
            }
        }
    }
    Ok(())
}

fn init(args: &InitArgs) -> Result<()> {
    let path = current_dir()?.join("apc.toml");
    if path.exists() && !args.force {
        bail!(
            "{} already exists (use --force to overwrite)",
            path.display()
        );
    }
    std::fs::write(&path, config::TEMPLATE).with_context(|| format!("write {}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

/// The files consulted and whether each one was loaded.
type Checked = Vec<(Source, String, &'static str)>;

/// Resolve the layers, plus the list of files consulted and whether each was loaded.
fn resolve(args: &Overrides) -> Result<(Loaded, Checked)> {
    let cwd = current_dir()?;
    let user =
        ProjectDirs::from("dev", "aplix", "aplix").map(|dirs| dirs.config_dir().join("apc.toml"));

    let entries: [(Source, Option<PathBuf>); 3] = [
        (Source::User, user),
        (Source::Project, Some(cwd.join("apc.toml"))),
        (Source::Local, Some(cwd.join("apc.local.toml"))),
    ];

    let mut layers = vec![Layer::defaults()];
    let mut checked = Vec::new();
    for (source, entry) in &entries {
        let source = *source;
        let path = entry.as_deref();
        let label = path.map_or_else(
            || "(no home directory)".to_owned(),
            |path| path.display().to_string(),
        );
        match path {
            Some(path) if path.is_file() => {
                let text =
                    std::fs::read_to_string(path).with_context(|| format!("read {label}"))?;
                layers.push(Layer::parse(source, &label, &text)?);
                checked.push((source, label, "loaded"));
            }
            _ => checked.push((source, label, "not found")),
        }
    }

    layers.push(environment()?);
    if let Some(flags) = flags(args)? {
        layers.push(flags);
    }
    Ok((config::resolve(&layers)?, checked))
}

fn show_config(args: &Overrides) -> Result<()> {
    let (loaded, checked) = resolve(args)?;

    let rows = [
        ("provider.name", loaded.config.provider.name.clone()),
        ("provider.model", loaded.config.provider.model.clone()),
        (
            "provider.base_url",
            loaded
                .config
                .provider
                .base_url
                .clone()
                .unwrap_or_else(|| "-".to_owned()),
        ),
        (
            "decision.threshold",
            loaded.config.decision.threshold.to_string(),
        ),
        (
            "decision.endpoint",
            loaded
                .config
                .decision
                .endpoint
                .clone()
                .unwrap_or_else(|| "-".to_owned()),
        ),
        ("telemetry", loaded.config.telemetry.to_string()),
    ];
    let rows: Vec<(String, String, Source)> = rows
        .into_iter()
        .map(|(key, value)| {
            let source = loaded.sources.get(key).copied().unwrap_or(Source::Defaults);
            (key.to_owned(), value, source)
        })
        .collect();

    let label_width = checked
        .iter()
        .map(|(_, label, _)| label.len())
        .max()
        .unwrap_or(0);
    println!("config files");
    for (source, label, state) in &checked {
        println!("  {:<7} {label:<label_width$}  {state}", source.label());
    }

    let key_width = rows.iter().map(|(key, _, _)| key.len()).max().unwrap_or(0);
    let value_width = rows
        .iter()
        .map(|(_, value, _)| value.len())
        .max()
        .unwrap_or(0);
    println!();
    for (key, value, source) in &rows {
        println!(
            "{key:<key_width$}  {value:<value_width$}  {}",
            source.label()
        );
    }
    Ok(())
}

fn current_dir() -> Result<PathBuf> {
    std::env::current_dir().context("cannot determine the current directory")
}

/// The env layer: `APC_*` variables, typed by the key they target.
fn environment() -> Result<Layer> {
    let pairs = ENV_KEYS.iter().filter_map(|(var, key)| {
        let text = std::env::var(var).ok()?;
        if text.is_empty() {
            return None;
        }
        Some(((*key).to_owned(), env_value(key, &text)))
    });
    Ok(Layer::from_pairs(Source::Env, "environment", pairs)?)
}

fn env_value(key: &str, text: &str) -> toml::Value {
    match key {
        "telemetry" => config::boolean(text)
            .map(toml::Value::Boolean)
            .unwrap_or_else(|| text.into()),
        "decision.threshold" => text
            .trim()
            .parse()
            .map(toml::Value::Float)
            .unwrap_or_else(|_| text.into()),
        _ => config::scalar(text),
    }
}

/// The flag layer, or `None` when no override was passed.
fn flags(args: &Overrides) -> Result<Option<Layer>> {
    let mut pairs = Vec::new();
    if let Some(name) = &args.provider {
        pairs.push((
            "provider.name".to_owned(),
            toml::Value::String(name.clone()),
        ));
    }
    if let Some(model) = &args.model {
        pairs.push((
            "provider.model".to_owned(),
            toml::Value::String(model.clone()),
        ));
    }
    if let Some(threshold) = args.permission_threshold {
        pairs.push((
            "decision.threshold".to_owned(),
            toml::Value::Float(threshold),
        ));
    }
    if pairs.is_empty() {
        return Ok(None);
    }
    Ok(Some(Layer::from_pairs(
        Source::Flag,
        "command line",
        pairs,
    )?))
}

#[cfg(test)]
mod tests {
    use super::apply_skill;
    use apc_core::chat::Message;

    /// A throwaway directory: no `tempfile` dependency for two tests.
    fn sandbox(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("apc-skill-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("skills")).expect("create the sandbox");
        dir
    }

    #[test]
    fn a_matching_skill_heads_the_conversation_and_replaces_the_previous_one() {
        let dir = sandbox("head");
        std::fs::write(dir.join("skills/generate_code.md"), "write tests first\n")
            .expect("write the skill");
        std::fs::write(dir.join("skills/explain_code.md"), "answer in French\n")
            .expect("write the skill");

        let mut messages = vec![Message::user("older")];
        apply_skill(&mut messages, &dir, "écris une fonction");
        assert_eq!(messages[0], Message::system("write tests first"));
        apply_skill(&mut messages, &dir, "explique ce code");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0], Message::system("answer in French"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_skill_file_means_no_system_message() {
        let dir = sandbox("missing");
        let mut messages = vec![Message::user("hello")];
        apply_skill(&mut messages, &dir, "review this");
        assert_eq!(messages, vec![Message::user("hello")]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
