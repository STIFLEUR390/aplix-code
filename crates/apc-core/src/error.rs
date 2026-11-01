//! Errors and the `Result` alias shared by every `apc` crate.

/// Result alias used across Aplix Code.
pub type Result<T> = std::result::Result<T, Error>;

/// Library errors. `apc-cli` adds path and user-facing context with `anyhow`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A configuration document is not valid TOML, or does not match the schema.
    #[error("{path}: {source}")]
    Config {
        /// Where the document came from (`apc.toml`, `environment`, ...).
        path: String,
        #[source]
        source: toml::de::Error,
    },

    /// `decision.threshold` is outside `0.0..=1.0`.
    #[error("decision.threshold must be between 0 and 1, got {0}")]
    Threshold(f64),

    /// A provider call could not be made or returned something unusable.
    #[error("{0}")]
    Provider(String),

    /// A tool refused or failed; the text goes back to the model as the call result.
    #[error("{0}")]
    Tool(String),
    /// The local decision backend failed; the caller falls back to the heuristics.
    #[error("{0}")]
    Decision(String),

    /// The model store could not satisfy a request (unknown id, bad checksum, short download).
    #[error("{0}")]
    Model(String),

    /// Session memory could not be read or written (`{path}: {source}` from SQLite).
    #[error("{0}")]
    Memory(String),

    /// The platform exposes no user data directory.
    #[error("cannot locate the user data directory for this platform")]
    NoDataDir,

    /// A session file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file involved.
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// A session file holds a line that is not a message.
    #[error("{path}: {source}")]
    Json {
        /// The file involved.
        path: String,
        #[source]
        source: serde_json::Error,
    },

    /// A provider needs an API key that is not in the environment.
    #[error("provider `{provider}` needs an API key in APC_API_KEY or {variable}")]
    MissingKey {
        /// Provider name as written in `apc.toml`.
        provider: String,
        /// Environment variable that would supply it.
        variable: &'static str,
    },
}
