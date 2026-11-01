//! Provider-neutral chat types.
//!
//! Every adapter speaks these; the CLI prints them, the agent walks them.

use std::pin::Pin;

use futures_core::Stream;

use crate::Result;

/// Who wrote a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Instructions for the whole conversation.
    System,
    /// The user.
    User,
    /// The model.
    Assistant,
}

impl Role {
    /// Wire name used by every provider request body.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

/// One message of a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Who wrote it.
    pub role: Role,
    /// Plain text body.
    pub content: String,
}

impl Message {
    /// A system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    /// A user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    /// An assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// One streamed step of a completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEvent {
    /// A piece of text, in order.
    Delta(String),
    /// The model finished; the stream ends after this.
    Done,
}

/// Streamed completion. Errors mid-stream are reported in-band.
pub type ChatStream = Pin<Box<dyn Stream<Item = Result<ChatEvent>> + Send>>;
