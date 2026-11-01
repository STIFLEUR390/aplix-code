#![forbid(unsafe_code)]
//! The agent loop: the model asks for tools, the tools answer, until it stops asking.

use std::sync::Arc;

use apc_core::chat::{ChatEvent, Message};
use apc_core::error::Error;
use apc_core::Result;
use apc_traits::{ModelAdapter, Tool};
use futures_util::StreamExt;
use tracing::{debug, warn};

/// Tool rounds one prompt may take before the loop gives up.
pub const MAX_STEPS: usize = 8;

/// Drives one conversation.
pub struct Agent {
    adapter: Arc<dyn ModelAdapter>,
    tools: Vec<Arc<dyn Tool>>,
}

impl Agent {
    /// `tools` are what the model may call; they are also what it is told about.
    pub fn new(adapter: Arc<dyn ModelAdapter>, tools: Vec<Arc<dyn Tool>>) -> Self {
        Self { adapter, tools }
    }

    /// Run until the model answers without asking for a tool, appending every turn to `conversation`.
    ///
    /// `on_event` receives the stream as it arrives, so the caller can print while it happens.
    pub async fn run<F>(&self, conversation: &mut Vec<Message>, mut on_event: F) -> Result<()>
    where
        F: FnMut(&ChatEvent),
    {
        let specs = self
            .tools
            .iter()
            .map(|tool| tool.spec())
            .collect::<Vec<_>>();

        for _ in 0..=MAX_STEPS {
            let stream = self.adapter.chat(conversation, &specs).await?;
            let (text, calls) = drain(stream, &mut on_event).await?;
            if calls.is_empty() {
                conversation.push(Message::assistant(text));
                return Ok(());
            }
            conversation.push(Message::assistant_calls(text, calls.clone()));
            for call in calls {
                let result = self.execute(&call).await;
                conversation.push(Message::tool_result(call.id, result));
            }
        }
        warn!(steps = MAX_STEPS, "agent stopped without a final answer");
        Err(Error::Tool(format!(
            "stopped after {MAX_STEPS} tool steps without a final answer"
        )))
    }

    /// Run one tool call. A failure is an answer to the model, not a crash for the user.
    async fn execute(&self, call: &apc_core::chat::ToolCall) -> String {
        let Some(tool) = self.tools.iter().find(|tool| tool.name() == call.name) else {
            warn!(tool = %call.name, "unknown tool");
            return format!("unknown tool `{}`", call.name);
        };
        let raw = call.arguments.clone();
        let args = serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({ "raw": raw }));
        // Names and outcomes only: never arguments, which can hold file contents.
        debug!(tool = %call.name, "tool call");
        match tool.execute(args).await {
            Ok(text) => text,
            Err(error) => {
                warn!(tool = %call.name, %error, "tool failed");
                format!("{error}")
            }
        }
    }
}

/// Collect one streamed turn: the text so far and the tool calls it asked for.
async fn drain<F>(
    mut stream: apc_core::chat::ChatStream,
    on_event: &mut F,
) -> Result<(String, Vec<apc_core::chat::ToolCall>)>
where
    F: FnMut(&ChatEvent),
{
    let mut text = String::new();
    let mut calls = Vec::new();
    while let Some(event) = stream.next().await {
        match event? {
            ChatEvent::Delta(delta) => {
                text.push_str(&delta);
                on_event(&ChatEvent::Delta(delta));
            }
            ChatEvent::ToolCall(call) => {
                on_event(&ChatEvent::ToolCall(call.clone()));
                calls.push(call);
            }
            ChatEvent::Done => break,
        }
    }
    Ok((text, calls))
}
