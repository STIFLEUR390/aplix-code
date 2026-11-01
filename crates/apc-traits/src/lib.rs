#![forbid(unsafe_code)]
//! Plugin traits — the contract every layer implements.

use apc_core::chat::{ChatStream, Message};
use apc_core::Result;

/// An LLM provider.
///
/// Streaming is part of the contract from v0.2 (SPEC.md §4): callers never wait for a full
/// completion before printing.
#[async_trait::async_trait]
#[allow(clippy::double_must_use)] // the generated future is already `#[must_use]`
pub trait ModelAdapter: Send + Sync {
    /// Stream one completion for the whole conversation.
    ///
    /// Connection and status errors surface here; errors after the stream starts arrive as items.
    async fn chat(&self, messages: &[Message]) -> Result<ChatStream>;

    /// Model identifier, for logs and diagnostics.
    fn model(&self) -> &str;
}
