#![forbid(unsafe_code)]
//! The `apc` binary: composition root for every layer.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use directories::ProjectDirs;

use apc_core::chat::{ChatEvent, Message};
use apc_core::config::{self, Layer, Loaded, Source};
use apc_memory::JsonlStore;
use apc_traits::{Approver, DecisionProvider, MemoryStore};

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
    }
}

fn overrides(command: &Command) -> Option<&Overrides> {
    match command {
        Command::Init(_) => None,
        Command::Config(args) => Some(args),
        Command::Run(args) => Some(&args.overrides),
        Command::Chat(args) => Some(&args.overrides),
        Command::Model(_) => None,
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
    let mut messages = vec![Message::user(args.prompt.join(" "))];
    agent.run(&mut messages, print_delta).await?;
    println!();
    Ok(())
}

async fn chat(args: &ChatArgs) -> Result<()> {
    let (loaded, _) = resolve(&args.overrides)?;
    let store = JsonlStore::new()?;
    let session = opened_session(&store, args.r#continue).await?;
    let messages = store.load(Some(&session)).await?;

    if !tui::available() {
        return repl(loaded.config, store, session, messages).await;
    }

    let (approver, asks) = tui::TuiApprover::channel();
    let agent = agent(&loaded.config, std::sync::Arc::new(approver))?;
    tui::chat(std::sync::Arc::new(agent), session, messages, asks).await
}

/// Resumes the latest session or starts a new one.
async fn opened_session(store: &JsonlStore, resume: bool) -> Result<apc_core::chat::SessionId> {
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
    store: JsonlStore,
    session: apc_core::chat::SessionId,
    mut messages: Vec<Message>,
) -> Result<()> {
    let agent = agent(&config, std::sync::Arc::new(StdinApprover))?;

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

fn agent(
    config: &config::Config,
    approver: std::sync::Arc<dyn Approver>,
) -> Result<apc_agent::Agent> {
    let adapter: std::sync::Arc<dyn apc_traits::ModelAdapter> =
        std::sync::Arc::from(apc_models::build(&config.provider)?);
    let decision = decision_provider(config.decision.endpoint.as_deref());
    let tools = apc_plugins::default_tools(
        std::env::current_dir()?,
        decision,
        approver,
        config.decision.threshold,
    );
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
