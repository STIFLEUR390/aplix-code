#![forbid(unsafe_code)]
//! The `apc` binary: composition root for every layer.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use directories::ProjectDirs;

use apc_core::config::{self, Layer, Loaded, Source};

mod telemetry;

/// `apc --version` prints the CLI version and the decision model (SPEC.md §2.1).
const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "\ndecision model: none (heuristic fallback)"
);

/// `APC_*` variables that override configuration (the env layer).
const ENV_KEYS: &[(&str, &str)] = &[
    ("APC_PROVIDER", "provider.name"),
    ("APC_MODEL", "provider.model"),
    ("APC_PERMISSION_THRESHOLD", "decision.threshold"),
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
    Config(ConfigArgs),
}

#[derive(Args)]
struct InitArgs {
    /// Overwrite an existing apc.toml.
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
struct ConfigArgs {
    /// Override the provider (flag layer).
    #[arg(long)]
    provider: Option<String>,
    /// Override the model (flag layer).
    #[arg(long)]
    model: Option<String>,
    /// Override the destructive-command threshold (flag layer).
    #[arg(long, value_name = "P")]
    permission_threshold: Option<f64>,
}

fn main() -> ExitCode {
    #[cfg(debug_assertions)]
    load_dotenv(); // dev only: a release build never reads a stray .env

    let cli = Cli::parse();
    let _sentry = telemetry::init(telemetry_enabled(&cli));

    match run(cli) {
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

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Init(args) => init(&args),
        Command::Config(args) => show_config(&args),
    }
}

/// Telemetry needs the config flag, and `apc init` is about to write that very config.
fn telemetry_enabled(cli: &Cli) -> bool {
    match &cli.command {
        Command::Init(_) => telemetry::wants_from_env(),
        Command::Config(args) => resolve(args).map_or_else(
            |_| telemetry::wants_from_env(),
            |(loaded, _)| loaded.config.telemetry,
        ),
    }
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
fn resolve(args: &ConfigArgs) -> Result<(Loaded, Checked)> {
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

fn show_config(args: &ConfigArgs) -> Result<()> {
    let (loaded, checked) = resolve(args)?;

    let rows = [
        ("provider.name", loaded.config.provider.name.clone()),
        ("provider.model", loaded.config.provider.model.clone()),
        (
            "decision.threshold",
            loaded.config.decision.threshold.to_string(),
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
fn flags(args: &ConfigArgs) -> Result<Option<Layer>> {
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
