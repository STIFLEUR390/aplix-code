//! Local logs, plus opt-in Sentry reporting (SPEC.md §3.4, §8.9).
//!
//! Two gates for Sentry, both required: `telemetry = true` in the config and a DSN in the
//! environment. Without either no client is built at all, so nothing can leave the process. A DSN
//! is only ever read from the environment, never from a committed file.
//!
//! Local `tracing` output is always on (stderr, `WARN` by default, `APC_LOG=debug` to widen it).

use std::io;
use std::str::FromStr;

use tracing::{Level, Subscriber};

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

pub fn init(enabled: bool) -> Guard {
    #[cfg(feature = "telemetry")]
    {
        init_with(enabled, dsn())
    }
    #[cfg(not(feature = "telemetry"))]
    {
        init_with(enabled, None)
    }
}

/// The subscriber is installed once, whatever the telemetry gate says: a process without a DSN
/// still gets local logs.
fn init_with(
    enabled: bool,
    #[cfg_attr(not(feature = "telemetry"), allow(unused_variables))] dsn: Option<String>,
) -> Guard {
    let level = log_level();
    let fmt = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(io::stderr)
        .with_target(false)
        .finish();

    #[cfg(feature = "telemetry")]
    if let (true, Some(dsn)) = (enabled, dsn) {
        use tracing_subscriber::layer::SubscriberExt;
        let guard = sentry::init((
            dsn,
            sentry::ClientOptions {
                release: Some(env!("CARGO_PKG_VERSION").into()),
                // Usage metrics are out of scope (SPEC.md §9.8): errors only.
                auto_session_tracking: false,
                ..Default::default()
            },
        ));
        // Only warnings and errors become breadcrumbs: no prompts, no file contents, no completions.
        let sentry_layer = sentry::integrations::tracing::layer().event_filter(|metadata| {
            match *metadata.level() {
                Level::WARN | Level::ERROR => sentry::integrations::tracing::EventFilter::Event,
                _ => sentry::integrations::tracing::EventFilter::Ignore,
            }
        });
        let subscriber: Box<dyn Subscriber + Send + Sync> = Box::new(fmt.with(sentry_layer));
        tracing::subscriber::set_global_default(subscriber).ok();
        return Some(guard);
    }

    let subscriber: Box<dyn Subscriber + Send + Sync> = Box::new(fmt);
    tracing::subscriber::set_global_default(subscriber).ok();
    #[cfg(feature = "telemetry")]
    return None;
    #[cfg(not(feature = "telemetry"))]
    {
        let _ = enabled;
    }
}

/// `APC_LOG` (`error`, `warn`, `info`, `debug`, `trace`), `WARN` otherwise.
fn log_level() -> Level {
    std::env::var("APC_LOG")
        .ok()
        .map_or(Level::WARN, |text| parse_level(&text))
}

fn parse_level(text: &str) -> Level {
    Level::from_str(text.trim()).unwrap_or(Level::WARN)
}

#[cfg(feature = "telemetry")]
pub fn capture(error: &anyhow::Error) {
    let error: &(dyn std::error::Error + Send + Sync + 'static) = error.as_ref();
    // No-op when the client is disabled.
    sentry::capture_error(error);
}

#[cfg(not(feature = "telemetry"))]
pub fn capture(_error: &anyhow::Error) {}

#[cfg(all(test, feature = "telemetry"))]
mod tests {
    use super::*;

    #[test]
    fn a_wanted_telemetry_without_a_dsn_builds_no_client() {
        assert!(init_with(true, None).is_none());
    }

    #[test]
    fn a_disabled_telemetry_ignores_the_dsn() {
        assert!(init_with(false, Some("http://key@localhost/1".to_owned())).is_none());
    }

    #[test]
    fn the_log_level_is_read_case_insensitively() {
        assert_eq!(parse_level("debug"), Level::DEBUG);
        assert_eq!(parse_level(" INFO "), Level::INFO);
        assert_eq!(parse_level("nonsense"), Level::WARN);
    }
}
