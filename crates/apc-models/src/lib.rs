#![forbid(unsafe_code)]
//! HTTP adapters for the supported LLM providers.
//!
//! SPEC.md §8.10: `genai` was dropped. Every provider we target speaks either the OpenAI
//! chat-completions dialect or the Anthropic messages dialect, so a few hundred lines of
//! `reqwest` beat a dependency that brings its own TLS stack, auth resolvers and provider SDKs.

mod sse;

use std::collections::VecDeque;
use std::pin::Pin;

use apc_core::chat::{ChatEvent, ChatStream, Message, Role, ToolCall};
use apc_core::config::Provider;
use apc_core::tool::ToolSpec;
use apc_core::{Error, Result};
use apc_traits::ModelAdapter;
use futures_util::stream::{self, Stream, StreamExt};
use serde_json::{json, Value};

use crate::sse::Sse;

/// Anthropic requires an explicit bound; ignored by the OpenAI dialect.
const MAX_TOKENS: u32 = 4096;

/// Wire protocol a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// `POST {base}/chat/completions`.
    OpenAi,
    /// `POST {base}/messages`.
    Anthropic,
}

/// Build the adapter named by `[provider]`.
///
/// The API key is read from the environment only — `APC_API_KEY` first, then the provider's own
/// variable — never from `apc.toml`.
pub fn build(provider: &Provider) -> Result<Box<dyn ModelAdapter>> {
    let (dialect, default_base, key_variable) = defaults(&provider.name)?;

    let key = std::env::var("APC_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
        .or_else(|| {
            key_variable.and_then(|variable| {
                std::env::var(variable)
                    .ok()
                    .filter(|key| !key.trim().is_empty())
            })
        });
    if key.is_none() {
        if let Some(variable) = key_variable {
            return Err(Error::MissingKey {
                provider: provider.name.clone(),
                variable,
            });
        }
    }

    let base_url = provider
        .base_url
        .clone()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| default_base.to_owned());

    Ok(Box::new(Http {
        client: reqwest::Client::new(),
        base_url,
        key,
        model: provider.model.clone(),
        dialect,
    }))
}

/// `(dialect, default base url, key variable)` for a provider name.
fn defaults(name: &str) -> Result<(Dialect, &'static str, Option<&'static str>)> {
    Ok(match name {
        "openai" => (
            Dialect::OpenAi,
            "https://api.openai.com/v1",
            Some("OPENAI_API_KEY"),
        ),
        "anthropic" => (
            Dialect::Anthropic,
            "https://api.anthropic.com/v1",
            Some("ANTHROPIC_API_KEY"),
        ),
        "deepseek" => (
            Dialect::OpenAi,
            "https://api.deepseek.com/v1",
            Some("DEEPSEEK_API_KEY"),
        ),
        "openrouter" => (
            Dialect::OpenAi,
            "https://openrouter.ai/api/v1",
            Some("OPENROUTER_API_KEY"),
        ),
        "groq" => (
            Dialect::OpenAi,
            "https://api.groq.com/openai/v1",
            Some("GROQ_API_KEY"),
        ),
        "ollama" => (Dialect::OpenAi, "http://localhost:11434/v1", None),
        "gemini" => {
            return Err(Error::Provider(
                "gemini needs its own dialect, not implemented in v0.2 (SPEC.md §8.10)".to_owned(),
            ))
        }
        other => {
            return Err(Error::Provider(format!(
                "unknown provider `{other}` (openai | anthropic | deepseek | openrouter | groq | ollama)"
            )))
        }
    })
}

#[derive(Debug)]
struct Http {
    client: reqwest::Client,
    base_url: String,
    key: Option<String>,
    model: String,
    dialect: Dialect,
}

impl Http {
    fn endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        match self.dialect {
            Dialect::OpenAi => format!("{base}/chat/completions"),
            Dialect::Anthropic => format!("{base}/messages"),
        }
    }
}

#[async_trait::async_trait]
impl ModelAdapter for Http {
    async fn chat(&self, messages: &[Message], tools: &[ToolSpec]) -> Result<ChatStream> {
        let request = self
            .client
            .post(self.endpoint())
            .header("accept", "text/event-stream")
            .json(&body(self.dialect, &self.model, messages, tools));

        let request = match (self.dialect, &self.key) {
            (Dialect::OpenAi, Some(key)) => request.bearer_auth(key),
            (Dialect::Anthropic, Some(key)) => request
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01"),
            _ => request,
        };

        let response = request
            .send()
            .await
            .map_err(|error| Error::Provider(format!("{}: {error}", self.base_url)))?;

        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_owned();
        if status.is_success() && !content_type.is_empty() && !content_type.contains("event-stream")
        {
            // A gateway serving its own HTML page under the API path used to stream nothing at all.
            let text = response.text().await.unwrap_or_default();
            return Err(Error::Provider(format!(
                "{}: expected an event stream, got `{content_type}`: {}",
                self.base_url,
                truncate(&text, 200)
            )));
        }
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(Error::Provider(format!(
                "{} returned {status}: {}",
                self.model,
                truncate(&text, 300)
            )));
        }
        Ok(stream_from(response, self.dialect))
    }

    fn model(&self) -> &str {
        &self.model
    }
}

/// Request body for one dialect.
fn body(dialect: Dialect, model: &str, messages: &[Message], tools: &[ToolSpec]) -> Value {
    match dialect {
        Dialect::OpenAi => {
            let mut body = json!({
                "model": model,
                "stream": true,
                "messages": messages.iter().map(openai_message).collect::<Vec<_>>(),
            });
            if !tools.is_empty() {
                body["tools"] = tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.parameters,
                            },
                        })
                    })
                    .collect();
                body["tool_choice"] = json!("auto");
            }
            body
        }
        Dialect::Anthropic => {
            // Anthropic takes the system prompt as a top-level field, not a message.
            let system = messages
                .iter()
                .filter(|message| message.role == Role::System)
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            let mut body = json!({
                "model": model,
                "max_tokens": MAX_TOKENS,
                "stream": true,
                "messages": messages
                    .iter()
                    .filter(|message| message.role != Role::System)
                    .map(anthropic_message)
                    .collect::<Vec<_>>(),
            });
            if !system.is_empty() {
                body["system"] = json!(system);
            }
            if !tools.is_empty() {
                body["tools"] = tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "name": tool.name,
                            "description": tool.description,
                            "input_schema": tool.parameters,
                        })
                    })
                    .collect();
            }
            body
        }
    }
}

/// Arguments as an object; a streamed fragment can leave them invalid.
fn arguments_object(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| json!({}))
}

fn openai_message(message: &Message) -> Value {
    if message.role == Role::Tool {
        return json!({
            "role": "tool",
            "tool_call_id": message.tool_call_id,
            "content": message.content,
        });
    }
    if message.tool_calls.is_empty() {
        return json!({ "role": message.role.as_str(), "content": message.content });
    }
    json!({
        "role": message.role.as_str(),
        "content": message.content,
        "tool_calls": message
            .tool_calls
            .iter()
            .map(|call| {
                json!({
                    "id": call.id,
                    "type": "function",
                    "function": { "name": call.name, "arguments": call.arguments },
                })
            })
            .collect::<Vec<_>>(),
    })
}

fn anthropic_message(message: &Message) -> Value {
    if message.role == Role::Tool {
        return json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": message.tool_call_id,
                "content": message.content,
            }],
        });
    }
    if message.tool_calls.is_empty() {
        return json!({ "role": message.role.as_str(), "content": message.content });
    }
    let mut blocks = Vec::new();
    if !message.content.is_empty() {
        blocks.push(json!({ "type": "text", "text": message.content }));
    }
    blocks.extend(message.tool_calls.iter().map(|call| {
        json!({
            "type": "tool_use",
            "id": call.id,
            "name": call.name,
            "input": arguments_object(&call.arguments),
        })
    }));
    json!({ "role": "assistant", "content": blocks })
}

impl Dialect {
    /// Read one SSE payload: the events it carries, and whether the turn is over.
    ///
    /// Tool calls arrive in fragments, so they accumulate in `calls` and are emitted whole.
    fn absorb(self, payload: &str, calls: &mut Vec<ToolCall>) -> Result<(Vec<ChatEvent>, bool)> {
        let mut events = Vec::new();
        if payload == "[DONE]" {
            events.extend(drain_calls(calls));
            return Ok((events, true));
        }
        let value: Value = serde_json::from_str(payload)
            .map_err(|error| Error::Provider(format!("unreadable event `{payload}`: {error}")))?;

        match self {
            Dialect::OpenAi => {
                let choice = &value["choices"][0];
                if let Some(text) = choice["delta"]["content"].as_str() {
                    if !text.is_empty() {
                        events.push(ChatEvent::Delta(text.to_owned()));
                    }
                }
                if let Some(fragments) = choice["delta"]["tool_calls"].as_array() {
                    for fragment in fragments {
                        let index = fragment["index"].as_u64().unwrap_or(0) as usize;
                        while calls.len() <= index {
                            calls.push(ToolCall {
                                id: String::new(),
                                name: String::new(),
                                arguments: String::new(),
                            });
                        }
                        let call = &mut calls[index];
                        if let Some(id) = fragment["id"].as_str() {
                            call.id = id.to_owned();
                        }
                        if let Some(name) = fragment["function"]["name"].as_str() {
                            call.name.push_str(name);
                        }
                        if let Some(arguments) = fragment["function"]["arguments"].as_str() {
                            call.arguments.push_str(arguments);
                        }
                    }
                }
                if choice["finish_reason"].as_str() == Some("tool_calls") {
                    events.extend(drain_calls(calls));
                }
            }
            Dialect::Anthropic => match value["type"].as_str() {
                Some("content_block_start") => {
                    if value["content_block"]["type"].as_str() == Some("tool_use") {
                        calls.push(ToolCall {
                            id: value["content_block"]["id"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                            name: value["content_block"]["name"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                            arguments: String::new(),
                        });
                    }
                }
                Some("content_block_delta") => match value["delta"]["type"].as_str() {
                    Some("input_json_delta") => {
                        if let (Some(call), Some(fragment)) =
                            (calls.last_mut(), value["delta"]["partial_json"].as_str())
                        {
                            call.arguments.push_str(fragment);
                        }
                    }
                    Some("text_delta") => {
                        if let Some(text) = value["delta"]["text"]
                            .as_str()
                            .filter(|text| !text.is_empty())
                        {
                            events.push(ChatEvent::Delta(text.to_owned()));
                        }
                    }
                    _ => {}
                },
                Some("message_stop") => {
                    events.extend(drain_calls(calls));
                    return Ok((events, true));
                }
                Some("error") => {
                    let message = value["error"]["message"].as_str().unwrap_or(payload);
                    return Err(Error::Provider(message.to_owned()));
                }
                _ => {}
            },
        }
        Ok((events, false))
    }
}

fn drain_calls(calls: &mut Vec<ToolCall>) -> Vec<ChatEvent> {
    calls
        .drain(..)
        .filter(|call| !call.name.is_empty())
        .map(ChatEvent::ToolCall)
        .collect()
}

/// Re-shape the HTTP body stream into `ChatEvent`s.
///
/// A server that closes without `[DONE]` still ends with exactly one `Done`, after any tool calls.
fn stream_from(response: reqwest::Response, dialect: Dialect) -> ChatStream {
    type Body = Pin<Box<dyn Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>;
    let source: Body = Box::pin(response.bytes_stream());
    let state = (source, Sse::default(), VecDeque::new(), Vec::new());

    Box::pin(stream::unfold(Some(state), move |state| async move {
        let (mut source, mut sse, mut pending, mut calls) = state?;
        loop {
            if let Some(event) = pending.pop_front() {
                let last = event == ChatEvent::Done;
                let state = (!last).then_some((source, sse, pending, calls));
                return Some((Ok(event), state));
            }
            match source.next().await {
                Some(Ok(chunk)) => {
                    for payload in sse.push(&String::from_utf8_lossy(&chunk)) {
                        match dialect.absorb(&payload, &mut calls) {
                            Ok((events, done)) => {
                                pending.extend(events);
                                if done {
                                    pending.push_back(ChatEvent::Done);
                                }
                            }
                            Err(error) => return Some((Err(error), None)),
                        }
                    }
                }
                Some(Err(error)) => {
                    return Some((
                        Err(Error::Provider(format!("stream interrupted: {error}"))),
                        None,
                    ))
                }
                None => {
                    pending.extend(drain_calls(&mut calls));
                    pending.push_back(ChatEvent::Done);
                }
            }
        }
    }))
}

fn truncate(text: &str, limit: usize) -> String {
    let text = text.trim();
    match text.char_indices().nth(limit) {
        Some((index, _)) => format!("{}…", &text[..index]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(dialect: Dialect, payload: &str) -> (Vec<ChatEvent>, bool) {
        let mut calls = Vec::new();
        dialect.absorb(payload, &mut calls).expect("parses")
    }

    #[test]
    fn openai_body_carries_the_whole_conversation() {
        let messages = [Message::system("be brief"), Message::user("hi")];
        let body = body(Dialect::OpenAi, "auto", &messages, &[]);
        assert_eq!(body["model"], "auto");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "hi");
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn anthropic_body_hoists_the_system_prompt() {
        let messages = [Message::system("be brief"), Message::user("hi")];
        let body = body(Dialect::Anthropic, "claude", &messages, &[]);
        assert_eq!(body["system"], "be brief");
        assert_eq!(body["messages"].as_array().map(Vec::len), Some(1));
        assert_eq!(body["max_tokens"], MAX_TOKENS);
    }

    #[test]
    fn tools_are_offered_in_the_dialect_shape() {
        let tool = ToolSpec {
            name: "read_file".to_owned(),
            description: "read".to_owned(),
            parameters: json!({ "type": "object" }),
        };
        let openai = body(
            Dialect::OpenAi,
            "auto",
            &[Message::user("hi")],
            std::slice::from_ref(&tool),
        );
        assert_eq!(openai["tools"][0]["function"]["name"], "read_file");
        assert_eq!(openai["tool_choice"], "auto");
        let anthropic = body(
            Dialect::Anthropic,
            "claude",
            &[Message::user("hi")],
            &[tool],
        );
        assert_eq!(anthropic["tools"][0]["input_schema"]["type"], "object");
    }

    #[test]
    fn openai_events_become_deltas() {
        assert_eq!(
            events(
                Dialect::OpenAi,
                r#"{"choices":[{"delta":{"content":"hel"}}]}"#
            )
            .0,
            vec![ChatEvent::Delta("hel".to_owned())]
        );
        // Role-only and empty deltas are bookkeeping.
        assert!(events(
            Dialect::OpenAi,
            r#"{"choices":[{"delta":{"role":"assistant"}}]}"#
        )
        .0
        .is_empty());
        assert!(events(Dialect::OpenAi, "[DONE]").1);
    }

    #[test]
    fn openai_tool_call_fragments_become_one_call() {
        let mut calls = Vec::new();
        let (first, done) = Dialect::OpenAi
            .absorb(
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}"#,
                &mut calls,
            )
            .expect("parses");
        assert!(first.is_empty() && !done);
        let (second, _) = Dialect::OpenAi
            .absorb(
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"Cargo.toml\"}"}}]},"finish_reason":"tool_calls"}]}"#,
                &mut calls,
            )
            .expect("parses");
        assert_eq!(
            second,
            vec![ChatEvent::ToolCall(ToolCall {
                id: "call_1".to_owned(),
                name: "read_file".to_owned(),
                arguments: r#"{"path":"Cargo.toml"}"#.to_owned(),
            })]
        );
    }

    #[test]
    fn anthropic_events_become_deltas() {
        let delta = r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}"#;
        assert_eq!(
            events(Dialect::Anthropic, delta).0,
            vec![ChatEvent::Delta("hi".to_owned())]
        );
        assert!(events(Dialect::Anthropic, r#"{"type":"message_stop"}"#).1);
        assert!(events(Dialect::Anthropic, r#"{"type":"ping"}"#)
            .0
            .is_empty());
    }

    #[test]
    fn anthropic_tool_use_accumulates_its_json() {
        let mut calls = Vec::new();
        Dialect::Anthropic
            .absorb(
                r#"{"type":"content_block_start","content_block":{"type":"tool_use","id":"toolu_1","name":"list_dir"}}"#,
                &mut calls,
            )
            .expect("parses");
        let (events, _) = Dialect::Anthropic
            .absorb(
                r#"{"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"{}"},"index":1}"#,
                &mut calls,
            )
            .expect("parses");
        assert!(events.is_empty());
        let (done, finished) = Dialect::Anthropic
            .absorb(r#"{"type":"message_stop"}"#, &mut calls)
            .expect("parses");
        assert!(finished);
        assert_eq!(
            done,
            vec![ChatEvent::ToolCall(ToolCall {
                id: "toolu_1".to_owned(),
                name: "list_dir".to_owned(),
                arguments: "{}".to_owned(),
            })]
        );
    }

    #[test]
    fn an_anthropic_error_event_is_an_error() {
        let mut calls = Vec::new();
        let error = Dialect::Anthropic
            .absorb(
                r#"{"type":"error","error":{"message":"overloaded"}}"#,
                &mut calls,
            )
            .expect_err("reports");
        assert!(error.to_string().contains("overloaded"), "{error}");
    }

    #[test]
    fn unknown_providers_are_rejected() {
        let provider = Provider {
            name: "nope".to_owned(),
            ..Provider::default()
        };
        let error = build(&provider).err().expect("rejects");
        assert!(error.to_string().contains("unknown provider"), "{error}");
    }

    #[test]
    fn truncate_keeps_whole_characters() {
        assert_eq!(truncate("héllo", 2), "hé…");
        assert_eq!(truncate("short", 10), "short");
    }
}
