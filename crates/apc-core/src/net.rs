//! TLS setup shared by every HTTP client.

/// Installs the process-wide rustls provider.
///
/// reqwest is built with `rustls-no-provider` (aws-lc-rs cannot link on Ubuntu 22.04), so the
/// first `Client` of a process must find a provider already installed. The call is idempotent.
pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
