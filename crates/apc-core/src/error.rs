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

    /// A provider needs an API key that is not in the environment.
    #[error("provider `{provider}` needs an API key in APC_API_KEY or {variable}")]
    MissingKey {
        /// Provider name as written in `apc.toml`.
        provider: String,
        /// Environment variable that would supply it.
        variable: &'static str,
    },
}
