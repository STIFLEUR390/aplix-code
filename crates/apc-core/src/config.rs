//! Layered configuration.
//!
//! Precedence, lowest to highest: defaults < user < project < local < env < flags.
//! Every key keeps the layer that last wrote it, so `apc config` can explain itself.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Written by `apc init`. Also the reference for every supported key.
pub const TEMPLATE: &str = r#"# apc.toml — Aplix Code project configuration.
#
# Precedence, lowest to highest:
#   defaults < user (~/.config/aplix/apc.toml) < this file < apc.local.toml < env < flags
#
# API keys never go here: they are read from the environment
# (ANTHROPIC_API_KEY, OPENAI_API_KEY, GEMINI_API_KEY, ...).

# Send warnings, errors and panics to a Sentry-compatible DSN
# (APC_SENTRY_DSN or SENTRY_DSN). Off by default; APC_TELEMETRY=0 wins over a DSN.
telemetry = false

[provider]
# openai | anthropic | deepseek | openrouter | groq | ollama
name = "anthropic"
model = "claude-sonnet-4-5"
# endpoint override, e.g. a local gateway (APC_BASE_URL)
# base_url = "http://127.0.0.1:31415/v1"

[decision]
# Shell commands with P(destructive) above this threshold ask for confirmation.
threshold = 0.8
# local-jev sidecar used when apc is built with the `open-jev` feature
# (APC_DECISION_ENDPOINT). Without it the heuristics answer.
# endpoint = "http://127.0.0.1:8080"

[tools]
# SearXNG instance backing `web_search` (or export APC_SEARXNG_URL). Unset means the tool is
# not registered at all: no URL, no outbound request, and no error at startup.
# searxng_url = "http://127.0.0.1:8888"
# Bytes of a fetched page handed to the model; the rest is dropped.
fetch_max_bytes = 65536
"#;

/// Fully resolved configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Which LLM provider and model to talk to.
    pub provider: Provider,
    /// Local decision layer settings.
    pub decision: Decision,
    /// Local web tools (v1.2).
    pub tools: Tools,
    /// Opt-in Sentry-compatible error reporting (SPEC.md §3.4).
    pub telemetry: bool,
}

/// `[provider]` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Provider {
    /// Provider name understood by `apc-models`.
    pub name: String,
    /// Model identifier passed to that provider.
    pub model: String,
    /// Override the provider's default endpoint (local gateways, proxies).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

impl Default for Provider {
    fn default() -> Self {
        Self {
            name: "anthropic".to_owned(),
            model: "claude-sonnet-4-5".to_owned(),
            base_url: None,
        }
    }
}

/// `[decision]` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Decision {
    /// Above this destructive probability, a shell command asks for confirmation.
    pub threshold: f64,
    /// Root URL of a local-jev sidecar; only read by builds with the `open-jev` feature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

impl Default for Decision {
    fn default() -> Self {
        Self {
            threshold: 0.8,
            endpoint: None,
        }
    }
}

/// `[tools]` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tools {
    /// SearXNG base URL; without it `web_search` is not registered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub searxng_url: Option<String>,
    /// Bytes of a fetched page kept for the model.
    pub fetch_max_bytes: usize,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            searxng_url: None,
            fetch_max_bytes: 65_536,
        }
    }
}

/// What the agent is allowed to do, chosen per session by the chat view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// Read-only: `write_file` and `run_shell` are not even offered to the model.
    Plan,
    /// Every built-in tool.
    #[default]
    Build,
}

impl std::fmt::Display for Mode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Plan => "plan",
            Self::Build => "build",
        })
    }
}

impl Mode {
    /// `None` when the word is not a mode, so the caller can report it.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "plan" => Some(Self::Plan),
            "build" => Some(Self::Build),
            _ => None,
        }
    }
}

impl Config {
    /// Point the agent at another provider, dropping a base URL written for the previous one.
    pub fn use_provider(&mut self, name: &str, model: Option<&str>) {
        if name != self.provider.name {
            self.provider.base_url = None;
        }
        self.provider.name = name.to_owned();
        if let Some(model) = model {
            self.provider.model = model.to_owned();
        }
    }

    fn validate(&self) -> Result<()> {
        if !(0.0..=1.0).contains(&self.decision.threshold) {
            return Err(Error::Threshold(self.decision.threshold));
        }
        Ok(())
    }
}

/// Which layer supplied a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// Compiled-in defaults.
    Defaults,
    /// `~/.config/aplix/apc.toml`.
    User,
    /// `./apc.toml`.
    Project,
    /// `./apc.local.toml`, untracked.
    Local,
    /// `APC_*` environment variables.
    Env,
    /// Command-line flags.
    Flag,
}

impl Source {
    /// Name shown by `apc config`.
    pub fn label(self) -> &'static str {
        match self {
            Source::Defaults => "defaults",
            Source::User => "user",
            Source::Project => "project",
            Source::Local => "local",
            Source::Env => "env",
            Source::Flag => "flag",
        }
    }
}

/// One configuration layer.
#[derive(Debug, Clone)]
pub struct Layer {
    /// Where this layer sits in the precedence order.
    pub source: Source,
    /// Parsed document; partial, resolved against defaults on merge.
    pub table: toml::Table,
}

impl Layer {
    /// Built-in defaults, expressed as a layer so provenance needs no special case.
    pub fn defaults() -> Self {
        let table = toml::Value::try_from(Config::default())
            .ok()
            .and_then(|value| value.as_table().cloned())
            .unwrap_or_default();
        Self {
            source: Source::Defaults,
            table,
        }
    }

    /// Read one TOML document. `path` is only a label for error messages.
    pub fn parse(source: Source, path: &str, text: &str) -> Result<Self> {
        let table: toml::Table = toml::from_str(text).map_err(|source| Error::Config {
            path: path.to_owned(),
            source,
        })?;
        let layer = Self { source, table };
        layer.validate(path)?;
        Ok(layer)
    }

    /// Flat `dotted.key = value` overrides: environment variables and flags.
    pub fn from_pairs(
        source: Source,
        path: &str,
        pairs: impl IntoIterator<Item = (String, toml::Value)>,
    ) -> Result<Self> {
        let mut table = toml::Table::new();
        for (key, value) in pairs {
            set_path(&mut table, &key, value);
        }
        let layer = Self { source, table };
        layer.validate(path)?;
        Ok(layer)
    }

    /// Re-deserialise one layer so a typo or a bad type names the file it came from.
    fn validate(&self, path: &str) -> Result<()> {
        toml::Value::Table(self.table.clone())
            .try_into::<Config>()
            .map_err(|source| Error::Config {
                path: path.to_owned(),
                source,
            })?;
        Ok(())
    }
}

/// Result of merging every layer.
#[derive(Debug, Clone)]
pub struct Loaded {
    /// Effective configuration.
    pub config: Config,
    /// Dotted key (`decision.threshold`) to the layer that supplied the final value.
    pub sources: BTreeMap<String, Source>,
}

/// Merge layers lowest precedence first.
pub fn resolve(layers: &[Layer]) -> Result<Loaded> {
    let mut merged = toml::Table::new();
    let mut sources = BTreeMap::new();
    for layer in layers {
        deep_merge(&mut merged, &layer.table);
        collect_leaves(&layer.table, "", layer.source, &mut sources);
    }
    let config: Config = toml::Value::Table(merged)
        .try_into()
        .map_err(|source| Error::Config {
            path: "merged configuration".to_owned(),
            source,
        })?;
    config.validate()?;
    Ok(Loaded { config, sources })
}

/// Parse an environment-style string into a TOML scalar.
pub fn scalar(text: &str) -> toml::Value {
    let text = text.trim();
    if let Ok(int) = text.parse::<i64>() {
        return int.into();
    }
    if let Ok(float) = text.parse::<f64>() {
        return float.into();
    }
    if let Ok(flag) = text.parse::<bool>() {
        return flag.into();
    }
    text.into()
}

/// Parse `1/0/true/false/yes/no/on/off`, or `None` for anything else.
pub fn boolean(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn deep_merge(base: &mut toml::Table, over: &toml::Table) {
    for (key, value) in over {
        match (base.get_mut(key), value) {
            (Some(toml::Value::Table(base)), toml::Value::Table(over)) => deep_merge(base, over),
            _ => {
                base.insert(key.clone(), value.clone());
            }
        }
    }
}

fn collect_leaves(
    table: &toml::Table,
    prefix: &str,
    source: Source,
    out: &mut BTreeMap<String, Source>,
) {
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            toml::Value::Table(nested) if !nested.is_empty() => {
                collect_leaves(nested, &path, source, out);
            }
            _ => {
                out.insert(path, source);
            }
        }
    }
}

/// Write `dotted.key = value` into a document, creating the tables it needs.
pub fn set_path(table: &mut toml::Table, path: &str, value: toml::Value) {
    match path.split_once('.') {
        None => {
            table.insert(path.to_owned(), value);
        }
        Some((head, rest)) => {
            if !table.contains_key(head) {
                table.insert(head.to_owned(), toml::Value::Table(toml::Table::new()));
            }
            if let Some(toml::Value::Table(nested)) = table.get_mut(head) {
                set_path(nested, rest, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(source: Source, text: &str) -> Layer {
        Layer::parse(source, source.label(), text).expect("valid layer")
    }

    fn pair(source: Source, key: &str, value: toml::Value) -> Layer {
        Layer::from_pairs(source, source.label(), [(key.to_owned(), value)]).expect("valid layer")
    }

    #[test]
    fn template_matches_the_defaults() {
        let parsed = layer(Source::Project, TEMPLATE);
        let config: Config = toml::Value::Table(parsed.table).try_into().expect("schema");
        assert_eq!(config, Config::default());
    }

    #[test]
    fn switching_provider_drops_a_base_url_written_for_the_old_one() {
        let mut config = Config::default();
        config.provider.base_url = Some("http://127.0.0.1:31415/v1".to_owned());
        config.use_provider("ollama", Some("llama3"));
        assert_eq!(config.provider.name, "ollama");
        assert_eq!(config.provider.model, "llama3");
        assert_eq!(config.provider.base_url, None);
    }

    #[test]
    fn switching_model_keeps_the_provider_base_url() {
        let mut config = Config::default();
        config.provider.base_url = Some("http://127.0.0.1:31415/v1".to_owned());
        config.use_provider("anthropic", Some("claude-haiku"));
        assert_eq!(config.provider.model, "claude-haiku");
        assert_eq!(
            config.provider.base_url.as_deref(),
            Some("http://127.0.0.1:31415/v1")
        );
    }

    #[test]
    fn defaults_layer_covers_every_key() {
        let defaults = Layer::defaults();
        assert_eq!(defaults.table.len(), 4);
        assert_eq!(
            defaults.table["decision"]["threshold"].as_float(),
            Some(0.8)
        );
    }

    #[test]
    fn precedence_runs_from_defaults_to_flags() {
        let layers = [
            Layer::defaults(),
            layer(Source::User, "[provider]\nmodel = \"user-model\"\n"),
            layer(Source::Project, "[provider]\nname = \"project\"\n"),
            layer(Source::Local, "[decision]\nthreshold = 0.5\n"),
            pair(Source::Env, "provider.model", "env-model".into()),
            pair(Source::Flag, "decision.threshold", 0.9.into()),
        ];

        let loaded = resolve(&layers).expect("resolves");
        assert_eq!(loaded.config.provider.name, "project");
        assert_eq!(loaded.config.provider.model, "env-model");
        assert_eq!(loaded.config.decision.threshold, 0.9);
        assert_eq!(loaded.sources["provider.name"], Source::Project);
        assert_eq!(loaded.sources["provider.model"], Source::Env);
        assert_eq!(loaded.sources["decision.threshold"], Source::Flag);
        assert_eq!(loaded.sources["telemetry"], Source::Defaults);
    }

    #[test]
    fn a_missing_key_keeps_the_lower_layer() {
        let layers = [
            Layer::defaults(),
            layer(Source::Project, "[provider]\nname = \"ollama\"\n"),
        ];
        let loaded = resolve(&layers).expect("resolves");
        assert_eq!(loaded.config.provider.model, Provider::default().model);
        assert_eq!(loaded.sources["provider.model"], Source::Defaults);
    }

    #[test]
    fn unknown_keys_name_their_file() {
        let error = Layer::parse(Source::Project, "apc.toml", "[provder]\nname = \"x\"\n")
            .expect_err("typo is rejected");
        assert!(error.to_string().contains("apc.toml"), "{error}");
        assert!(error.to_string().contains("provder"), "{error}");
    }

    #[test]
    fn threshold_outside_the_unit_range_is_rejected() {
        let error = resolve(&[layer(Source::Project, "[decision]\nthreshold = 1.5\n")])
            .expect_err("out of range");
        assert!(matches!(error, Error::Threshold(_)), "{error}");
    }

    #[test]
    fn nested_pairs_build_nested_tables() {
        let layers = [
            Layer::defaults(),
            pair(Source::Flag, "provider.model", "gpt-x".into()),
        ];
        let loaded = resolve(&layers).expect("resolves");
        assert_eq!(loaded.config.provider.model, "gpt-x");
        assert_eq!(loaded.sources["provider.model"], Source::Flag);
    }

    #[test]
    fn env_values_are_parsed_as_scalars() {
        assert_eq!(scalar("0.75"), toml::Value::Float(0.75));
        assert_eq!(scalar("42"), toml::Value::Integer(42));
        assert_eq!(scalar("true"), toml::Value::Boolean(true));
        assert_eq!(scalar("ollama"), toml::Value::String("ollama".into()));
        assert_eq!(boolean("0"), Some(false));
        assert_eq!(boolean("NO"), Some(false));
        assert_eq!(boolean("maybe"), None);
    }
}
