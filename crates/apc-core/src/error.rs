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
}
