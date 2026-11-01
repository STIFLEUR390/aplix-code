//! Opt-in Sentry error reporting (SPEC.md §3.4).
//!
//! Two gates, both required: `telemetry = true` in the config and a DSN in the environment.
//! Without either the client is disabled and nothing leaves the process. A DSN is only ever
//! read from the environment, never from a committed file.

/// Kept alive for the whole process: dropping it flushes queued events.
#[cfg(feature = "telemetry")]
pub type Guard = Option<sentry::ClientInitGuard>;

/// Compiled without the `telemetry` feature: nothing to hold.
#[cfg(not(feature = "telemetry"))]
pub type Guard = ();

/// `APC_SENTRY_DSN`, else `SENTRY_DSN`, else nothing.
#[cfg(feature = "telemetry")]
pub fn dsn() -> Option<String> {
    ["APC_SENTRY_DSN", "SENTRY_DSN"]
        .iter()
        .filter_map(|var| std::env::var(var).ok())
        .find(|value| !value.trim().is_empty())
}

/// The env layer, used when the config could not be read (that error is worth reporting).
pub fn wants_from_env() -> bool {
    std::env::var("APC_TELEMETRY")
        .ok()
        .and_then(|text| apc_core::config::boolean(&text))
        .unwrap_or(false)
}

#[cfg(feature = "telemetry")]
pub fn init(enabled: bool) -> Guard {
    if !enabled {
        return None;
    }
    let dsn = dsn()?;
    Some(sentry::init((
        dsn,
        sentry::ClientOptions {
            release: Some(env!("CARGO_PKG_VERSION").into()),
            // Usage metrics are out of scope (SPEC.md §9.8): errors only.
            auto_session_tracking: false,
            ..Default::default()
        },
    )))
}

#[cfg(not(feature = "telemetry"))]
pub fn init(_enabled: bool) -> Guard {}

#[cfg(feature = "telemetry")]
pub fn capture(error: &anyhow::Error) {
    let error: &(dyn std::error::Error + Send + Sync + 'static) = error.as_ref();
    // No-op when the client is disabled.
    sentry::capture_error(error);
}

#[cfg(not(feature = "telemetry"))]
pub fn capture(_error: &anyhow::Error) {}
