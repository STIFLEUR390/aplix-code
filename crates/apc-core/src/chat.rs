//! Provider-neutral chat types.
//!
//! Every adapter speaks these; the CLI prints them, the agent walks them.

use std::fmt;
use std::pin::Pin;

use futures_core::Stream;
use serde::{Deserialize, Serialize};

use crate::Result;

/// Who wrote a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Instructions for the whole conversation.
    System,
    /// The user.
    User,
    /// The model.
    Assistant,
    /// Output of a tool the model asked for.
    Tool,
}

impl Role {
    /// Wire name used by every provider request body.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// One message of a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Who wrote it.
    pub role: Role,
    /// Plain text body.
    pub content: String,
    /// Tools the model asked to run in this turn (assistant messages only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Which call this answers (tool messages only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// A tool invocation requested by the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-side identifier, echoed back with the result.
    pub id: String,
    /// Name of the tool to run.
    pub name: String,
    /// Arguments as a JSON object string, exactly as the model streamed them.
    pub arguments: String,
}

impl Message {
    /// A system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self::text(Role::System, content)
    }

    /// A user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self::text(Role::User, content)
    }

    /// An assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::text(Role::Assistant, content)
    }

    /// An assistant turn that asks for tools, with whatever it said alongside.
    pub fn assistant_calls(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_calls,
            tool_call_id: None,
        }
    }

    /// The output of a tool call, sent back to the model.
    pub fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(call_id.into()),
        }
    }

    fn text(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
}

/// One streamed step of a completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEvent {
    /// A piece of text, in order.
    Delta(String),
    /// The model asked to run a tool. Emitted before `Done`, after its text.
    ToolCall(ToolCall),
    /// The model finished; the stream ends after this.
    Done,
}

/// Streamed completion. Errors mid-stream are reported in-band.
pub type ChatStream = Pin<Box<dyn Stream<Item = Result<ChatEvent>> + Send>>;

/// Identifier of a persisted conversation. The store decides the format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionId(String);

impl SessionId {
    /// Wrap an id.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id as written in file names.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
