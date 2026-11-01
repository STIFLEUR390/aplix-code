#![forbid(unsafe_code)]
//! Plugin traits — the contract every layer implements.

use apc_core::chat::{ChatStream, Message, SessionId};
use apc_core::tool::ToolSpec;
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
    async fn chat(&self, messages: &[Message], tools: &[ToolSpec]) -> Result<ChatStream>;

    /// Model identifier, for logs and diagnostics.
    fn model(&self) -> &str;
}

/// Where a conversation is persisted.
///
/// Session memory is P0 (SPEC.md §2.5); long-term memory arrives in v1.3 behind the same trait.
#[async_trait::async_trait]
#[allow(clippy::double_must_use)] // the generated futures are already `#[must_use]`
pub trait MemoryStore: Send + Sync {
    /// Append one message to a session.
    async fn append(&self, session: &SessionId, message: &Message) -> Result<()>;

    /// Replay a session, or the most recent one when `session` is `None`.
    async fn load(&self, session: Option<&SessionId>) -> Result<Vec<Message>>;

    /// What earlier sessions said about `query`, best match first.
    ///
    /// The query is plain text: callers pass a prompt as written, not a search syntax.
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Recall>>;
}

/// A passage of an earlier session, as returned by [`MemoryStore::search`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recall {
    /// The session it was written in.
    pub session: SessionId,
    /// The message around the match, possibly cut.
    pub snippet: String,
}

/// A capability the model can invoke.
///
/// Every implementation must treat its arguments as untrusted: they were written by a model.
#[async_trait::async_trait]
#[allow(clippy::double_must_use)] // the generated futures are already `#[must_use]`
pub trait Tool: Send + Sync {
    /// Stable name the model calls.
    fn name(&self) -> &str;

    /// One-line purpose, shown to the model.
    fn description(&self) -> &str;

    /// JSON schema of the arguments object.
    fn schema(&self) -> serde_json::Value;

    /// Run the tool. `Err` is reported to the model as a failed call, never to the user as a crash.
    async fn execute(&self, args: serde_json::Value) -> Result<String>;

    /// What the provider is told about this tool.
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name().to_owned(),
            description: self.description().to_owned(),
            parameters: self.schema(),
        }
    }
}

/// The local decision engine: "is this dangerous?" / "what does the user want?".
///
/// The heuristic backend ships first (SPEC.md §8.1); `open-jev` arrives behind a feature flag.
#[async_trait::async_trait]
#[allow(clippy::double_must_use)] // the generated futures are already `#[must_use]`
pub trait DecisionProvider: Send + Sync {
    /// Probability in `0.0..=1.0` that the answer to `question` about `state` is "yes".
    async fn noul(&self, state: &str, question: &str) -> Result<f64>;

    /// Pick one of `options` for `state`, with the confidence behind the pick.
    async fn choice(&self, state: &str, question: &str, options: &[&str]) -> Result<(String, f64)>;
}

/// Asks the human before a guarded action runs (SPEC.md §2.3).
///
/// Blocking on purpose: the CLI reads the terminal, and there is nothing else to do meanwhile.
pub trait Approver: Send + Sync {
    /// `true` runs the action, `false` aborts it and reports the refusal to the model.
    fn confirm(&self, command: &str, risk: f64) -> Result<bool>;
}
