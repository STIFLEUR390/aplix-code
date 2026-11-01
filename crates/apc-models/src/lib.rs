#![forbid(unsafe_code)]
//! HTTP adapters for the supported LLM providers.
//!
//! SPEC.md §8.10: `genai` was dropped. Every provider we target speaks either the OpenAI
//! chat-completions dialect or the Anthropic messages dialect, so a few hundred lines of
//! `reqwest` beat a dependency that brings its own TLS stack, auth resolvers and provider SDKs.

mod sse;

use std::collections::VecDeque;
use std::pin::Pin;

use apc_core::chat::{ChatEvent, ChatStream, Message, Role};
use apc_core::config::Provider;
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
    async fn chat(&self, messages: &[Message]) -> Result<ChatStream> {
        let request = self
            .client
            .post(self.endpoint())
            .header("accept", "text/event-stream")
            .json(&body(self.dialect, &self.model, messages));

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
fn body(dialect: Dialect, model: &str, messages: &[Message]) -> Value {
    let conversation = |filter: fn(&Message) -> bool| -> Vec<Value> {
        messages
            .iter()
            .filter(|message| filter(message))
            .map(|message| json!({ "role": message.role.as_str(), "content": message.content }))
            .collect()
    };

    match dialect {
        Dialect::OpenAi => json!({
            "model": model,
            "stream": true,
            "messages": conversation(|_| true),
        }),
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
                "messages": conversation(|message| message.role != Role::System),
            });
            if !system.is_empty() {
                body["system"] = json!(system);
            }
            body
        }
    }
}

impl Dialect {
    /// Turn one SSE payload into a step, or `None` for bookkeeping events.
    fn event(self, payload: &str) -> Result<Option<ChatEvent>> {
        if payload == "[DONE]" {
            return Ok(Some(ChatEvent::Done));
        }
        let value: Value = serde_json::from_str(payload)
            .map_err(|error| Error::Provider(format!("unreadable event `{payload}`: {error}")))?;

        let text = |value: &Value| {
            value
                .as_str()
                .filter(|text| !text.is_empty())
                .map(|text| ChatEvent::Delta(text.to_owned()))
        };

        Ok(match self {
            Dialect::OpenAi => value["choices"]
                .get(0)
                .and_then(|choice| text(&choice["delta"]["content"])),
            Dialect::Anthropic => match value["type"].as_str() {
                Some("content_block_delta") => text(&value["delta"]["text"]),
                Some("message_stop") => Some(ChatEvent::Done),
                Some("error") => {
                    let message = value["error"]["message"].as_str().unwrap_or(payload);
                    return Err(Error::Provider(message.to_owned()));
                }
                _ => None,
            },
        })
    }
}

/// Re-shape the HTTP body stream into `ChatEvent`s.
///
/// A server that closes without `[DONE]` still ends with exactly one `Done`.
fn stream_from(response: reqwest::Response, dialect: Dialect) -> ChatStream {
    type Body = Pin<Box<dyn Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>;
    let source: Body = Box::pin(response.bytes_stream());
    let state = (source, Sse::default(), VecDeque::new());

    Box::pin(stream::unfold(Some(state), move |state| async move {
        let (mut source, mut sse, mut pending) = state?;
        loop {
            if let Some(event) = pending.pop_front() {
                let last = event == ChatEvent::Done;
                let state = (!last).then_some((source, sse, pending));
                return Some((Ok(event), state));
            }
            match source.next().await {
                Some(Ok(chunk)) => {
                    for payload in sse.push(&String::from_utf8_lossy(&chunk)) {
                        match dialect.event(&payload) {
                            Ok(Some(event)) => pending.push_back(event),
                            Ok(None) => {}
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
                None => return Some((Ok(ChatEvent::Done), None)),
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

    #[test]
    fn openai_body_carries_the_whole_conversation() {
        let messages = [Message::system("be brief"), Message::user("hi")];
        let body = body(Dialect::OpenAi, "auto", &messages);
        assert_eq!(body["model"], "auto");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "hi");
    }

    #[test]
    fn anthropic_body_hoists_the_system_prompt() {
        let messages = [Message::system("be brief"), Message::user("hi")];
        let body = body(Dialect::Anthropic, "claude", &messages);
        assert_eq!(body["system"], "be brief");
        assert_eq!(body["messages"].as_array().map(Vec::len), Some(1));
        assert_eq!(body["max_tokens"], MAX_TOKENS);
    }

    #[test]
    fn openai_events_become_deltas() {
        let payload = r#"{"choices":[{"delta":{"content":"hel"}}]}"#;
        assert_eq!(
            Dialect::OpenAi.event(payload).expect("parses"),
            Some(ChatEvent::Delta("hel".to_owned()))
        );
        // Role-only and empty deltas are bookkeeping.
        assert_eq!(
            Dialect::OpenAi
                .event(r#"{"choices":[{"delta":{"role":"assistant"}}]}"#)
                .expect("parses"),
            None
        );
        assert_eq!(
            Dialect::OpenAi.event("[DONE]").expect("parses"),
            Some(ChatEvent::Done)
        );
    }

    #[test]
    fn anthropic_events_become_deltas() {
        let delta = r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}"#;
        assert_eq!(
            Dialect::Anthropic.event(delta).expect("parses"),
            Some(ChatEvent::Delta("hi".to_owned()))
        );
        assert_eq!(
            Dialect::Anthropic
                .event(r#"{"type":"message_stop"}"#)
                .expect("parses"),
            Some(ChatEvent::Done)
        );
        assert_eq!(
            Dialect::Anthropic
                .event(r#"{"type":"ping"}"#)
                .expect("parses"),
            None
        );
    }

    #[test]
    fn an_anthropic_error_event_is_an_error() {
        let error = Dialect::Anthropic
            .event(r#"{"type":"error","error":{"message":"overloaded"}}"#)
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
