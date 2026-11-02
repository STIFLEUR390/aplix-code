#![forbid(unsafe_code)]
//! The agent loop: the model asks for tools, the tools answer, until it stops asking.

use std::sync::Arc;

use apc_core::chat::{ChatEvent, Message, Role};
use apc_core::error::Error;
use apc_core::Result;
use apc_traits::{ModelAdapter, Tool};
use futures_util::StreamExt;
use tracing::{debug, warn};

/// Tool rounds one prompt may take before the loop gives up.
pub const MAX_STEPS: usize = 8;

/// Percentage of `context.max_bytes` at which older turns are summarised.
///
/// 75% sits inside the 70–80% band asked for: enough room left to summarise and answer in the
/// same window, and a turn that grows mid-loop still stays under the budget.
pub const COMPACT_AT: usize = 75;

/// Drives one conversation.
pub struct Agent {
    adapter: Arc<dyn ModelAdapter>,
    tools: Vec<Arc<dyn Tool>>,
    context_bytes: usize,
}

impl Agent {
    /// `tools` are what the model may call; they are also what it is told about.
    ///
    /// `context_bytes` is the conversation budget (`context.max_bytes`); past 75% of it, the turns
    /// before the current prompt are summarised once into a system message.
    pub fn new(
        adapter: Arc<dyn ModelAdapter>,
        tools: Vec<Arc<dyn Tool>>,
        context_bytes: usize,
    ) -> Self {
        Self {
            adapter,
            tools,
            context_bytes,
        }
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

        // Set once per run: re-summarising every step would cost a second call per tool round.
        let mut summary: Option<(String, usize)> = None;

        for _ in 0..=MAX_STEPS {
            if summary.is_none() && self.bytes(conversation) > self.budget() {
                summary = self.summarize(conversation, &mut on_event).await;
            }
            // The compacted view keeps the system prompt and everything from the current prompt on,
            // so a tool result the model is answering stays visible.
            let compacted: Vec<Message> = match &summary {
                Some((text, from)) => {
                    let head = usize::from(
                        conversation
                            .first()
                            .is_some_and(|message| message.role == Role::System),
                    );
                    let mut view = conversation[..head].to_vec();
                    view.push(Message::system(format!(
                        "Earlier conversation, summarised automatically:\n{text}"
                    )));
                    view.extend_from_slice(&conversation[*from..]);
                    view
                }
                None => Vec::new(),
            };
            let sent: &[Message] = if summary.is_some() {
                &compacted
            } else {
                conversation
            };

            let stream = self.adapter.chat(sent, &specs).await?;
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

impl Agent {
    /// Bytes the model is sent, roughly: the text plus the arguments of every call.
    fn bytes(&self, conversation: &[Message]) -> usize {
        conversation
            .iter()
            .map(|message| {
                message.content.len()
                    + message
                        .tool_calls
                        .iter()
                        .map(|call| call.name.len() + call.arguments.len())
                        .sum::<usize>()
            })
            .sum()
    }

    /// Where compaction starts being worth it.
    fn budget(&self) -> usize {
        self.context_bytes / 100 * COMPACT_AT
    }

    /// Summarise everything before the current prompt. `None` means there is nothing to drop.
    ///
    /// Fails open: a provider that cannot summarise is not a reason to lose the turn, so the
    /// summary becomes a note saying how much was left out.
    async fn summarize<F>(
        &self,
        conversation: &[Message],
        on_event: &mut F,
    ) -> Option<(String, usize)>
    where
        F: FnMut(&ChatEvent),
    {
        let from = conversation
            .iter()
            .rposition(|message| message.role == Role::User)?;
        let head = usize::from(
            conversation
                .first()
                .is_some_and(|message| message.role == Role::System),
        );
        if from <= head {
            return None;
        }
        let dropped = &conversation[head..from];
        let mut prompt = Vec::with_capacity(dropped.len() + 1);
        prompt.push(Message::system(
            "Summarise the conversation below for your own later use: keep decisions, file paths, \
             values and open questions, drop the small talk. Answer with the summary only.",
        ));
        prompt.extend_from_slice(dropped);
        let text = match self.adapter.chat(&prompt, &[]).await {
            Ok(stream) => drain(stream, &mut |_| {}).await.map(|(text, _)| text),
            Err(error) => Err(error),
        };
        let summary = match text {
            Ok(text) if !text.trim().is_empty() => text,
            Ok(_) => format!(
                "{} earlier messages were dropped: nothing to summarise.",
                dropped.len()
            ),
            Err(error) => {
                warn!(%error, "compaction failed, dropping instead of summarising");
                format!(
                    "{} earlier messages were dropped to stay inside the context budget.",
                    dropped.len()
                )
            }
        };
        debug!(
            dropped = dropped.len(),
            bytes = self.bytes(conversation),
            "compacted"
        );
        on_event(&ChatEvent::Compacted(dropped.len()));
        Some((summary, from))
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
            ChatEvent::Compacted(_) => {}
            ChatEvent::Done => break,
        }
    }
    Ok((text, calls))
}

#[cfg(test)]
mod tests {
    use super::*;
    use apc_core::chat::ChatStream;
    use apc_core::tool::ToolSpec;
    use std::sync::Mutex;

    /// Answers "SUMMARY" when asked to summarise (no tools), "ok" otherwise; records every call.
    struct Stub {
        calls: Mutex<Vec<Vec<Message>>>,
        fail_summary: bool,
    }

    impl Stub {
        fn new(fail_summary: bool) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                fail_summary,
            }
        }
    }

    #[async_trait::async_trait]
    impl ModelAdapter for Stub {
        async fn chat(&self, messages: &[Message], _tools: &[ToolSpec]) -> Result<ChatStream> {
            self.calls.lock().expect("calls").push(messages.to_vec());
            let summary_call = messages
                .first()
                .is_some_and(|message| message.content.starts_with("Summarise the conversation"));
            let text = if summary_call {
                if self.fail_summary {
                    return Err(apc_core::error::Error::Tool("no summary".to_owned()));
                }
                "SUMMARY"
            } else {
                "ok"
            };
            Ok(Box::pin(futures_util::stream::iter(vec![
                Ok(ChatEvent::Delta(text.to_owned())),
                Ok(ChatEvent::Done),
            ])))
        }

        fn model(&self) -> &str {
            "stub"
        }
    }

    fn conversation(padding: usize) -> Vec<Message> {
        vec![
            Message::system("you are apc"),
            Message::user("é".repeat(padding)),
            Message::assistant("ok"),
            Message::user("next"),
        ]
    }

    #[tokio::test]
    async fn an_over_budget_conversation_is_summarised_before_the_model_sees_it() {
        let stub = Arc::new(Stub::new(false));
        let agent = Agent::new(stub.clone(), Vec::new(), 400);
        let mut messages = conversation(400);
        let mut events = Vec::new();
        agent
            .run(&mut messages, |event| events.push(event.clone()))
            .await
            .expect("runs");

        let calls = stub.calls.lock().expect("calls");
        assert_eq!(calls.len(), 2, "one summary call, then the answer");
        assert!(calls[0].len() == 3, "system prompt plus the two old turns");
        let sent = &calls[1];
        assert_eq!(sent.len(), 3, "system prompt, summary, current prompt");
        assert!(sent[1].content.contains("SUMMARY"), "{}", sent[1].content);
        assert_eq!(sent[2].content, "next");
        assert!(events.contains(&ChatEvent::Compacted(2)), "{events:?}");
        // The caller's conversation keeps every message (plus the answer): persistence sees the
        // whole session, only what the model was sent was summarised.
        assert_eq!(messages.len(), 5);
    }

    #[tokio::test]
    async fn a_broken_summary_keeps_the_turn_and_says_what_was_dropped() {
        let stub = Arc::new(Stub::new(true));
        let agent = Agent::new(stub.clone(), Vec::new(), 400);
        let mut messages = conversation(400);
        let mut events = Vec::new();
        let end = |event: &ChatEvent| events.push(event.clone());
        agent.run(&mut messages, end).await.expect("runs anyway");

        let calls = stub.calls.lock().expect("calls");
        assert!(
            calls[1][1]
                .content
                .contains("earlier messages were dropped"),
            "{}",
            calls[1][1].content
        );
    }

    #[tokio::test]
    async fn a_small_conversation_is_sent_as_it_is() {
        let stub = Arc::new(Stub::new(false));
        let agent = Agent::new(stub.clone(), Vec::new(), 262_144);
        let mut messages = conversation(10);
        agent.run(&mut messages, |_| {}).await.expect("runs");

        let calls = stub.calls.lock().expect("calls");
        assert_eq!(calls.len(), 1, "no summary call");
        assert_eq!(calls[0].len(), 4);
    }
}
