//! `local-jev` backend: asks a local model instead of counting patterns (SPEC.md §8.11).
//!
//! local-jev speaks TypeSafe Jev's decision API. Its `choice` route carries one probability per
//! option, which is exactly what the guard needs; `noul` classifies into a category rather than
//! answering yes/no, so a yes/no question is sent as a two-option `choice` and read back on the
//! `yes` probability — not on `confidence`, which only describes the winner.

use apc_core::{Error, Result};
use apc_traits::DecisionProvider;
use serde_json::{json, Value};

/// Below this gap between the two best options, the model is not choosing anything.
///
/// local-jev's zero-weight engine answers a flat 50/50 to every question. Reading that as "low
/// risk" would disable the guard on the very commands it exists for, so a flat answer is reported
/// as an error: `Cascade` then hands the question to the heuristics.
const NO_OPINION: f64 = 0.01;

/// Client for a local-jev (or TypeSafe Jev compatible) sidecar.
pub struct JevProvider {
    client: reqwest::Client,
    base_url: String,
}

impl JevProvider {
    /// `base_url` is the server root, e.g. `http://127.0.0.1:8080`.
    pub fn new(base_url: impl Into<String>) -> Self {
        apc_core::net::install_crypto_provider();
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_owned(),
        }
    }

    async fn choose(&self, state: &str, question: &str, options: &[&str]) -> Result<Value> {
        let url = format!("{}/v1/systemone/choice", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(&json!({ "state": state, "question": question, "options": options }))
            .send()
            .await
            .map_err(|error| Error::Decision(format!("{url}: {error}")))?;
        let status = response.status();
        let payload: Value = response
            .json()
            .await
            .map_err(|error| Error::Decision(format!("{url}: {error}")))?;
        if !status.is_success() {
            return Err(Error::Decision(format!("{url}: {status}: {payload}")));
        }
        if flat(&payload) {
            return Err(Error::Decision(
                "local-jev answered 50/50, so it has no weights loaded".to_owned(),
            ));
        }
        Ok(payload)
    }
}

/// True when the two best probabilities are indistinguishable.
fn flat(payload: &Value) -> bool {
    let mut probabilities: Vec<f64> = payload["probabilities"]
        .as_array()
        .map(|all| {
            all.iter()
                .filter_map(|entry| entry["probability"].as_f64())
                .collect()
        })
        .unwrap_or_default();
    if probabilities.len() < 2 {
        return false;
    }
    probabilities.sort_by(|a, b| b.total_cmp(a));
    probabilities[0] - probabilities[1] < NO_OPINION
}

#[async_trait::async_trait]
impl DecisionProvider for JevProvider {
    async fn noul(&self, state: &str, question: &str) -> Result<f64> {
        let payload = self.choose(state, question, &["no", "yes"]).await?;
        payload["probabilities"]
            .as_array()
            .and_then(|all| {
                all.iter()
                    .find(|entry| entry["option"] == "yes")
                    .and_then(|entry| entry["probability"].as_f64())
            })
            .ok_or_else(|| Error::Decision(format!("no `yes` probability in {payload}")))
    }

    async fn choice(&self, state: &str, question: &str, options: &[&str]) -> Result<(String, f64)> {
        let payload = self.choose(state, question, options).await?;
        let choice = payload["choice"]
            .as_str()
            .ok_or_else(|| Error::Decision(format!("no choice in {payload}")))?;
        Ok((
            choice.to_owned(),
            payload["confidence"].as_f64().unwrap_or(0.0),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn serving(status: &'static str, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let response = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        });
        url
    }

    #[tokio::test]
    async fn noul_reads_the_yes_probability_not_the_confidence() {
        let body = r#"{"choice":"no","confidence":0.9,"probabilities":[{"option":"no","probability":0.7,"logit":1.0},{"option":"yes","probability":0.3,"logit":-0.8}],"latency_ms":1.0,"cached":false}"#;
        let provider = JevProvider::new(serving("200 OK", body));
        assert_eq!(provider.noul("ls", "destructive?").await.unwrap(), 0.3);
    }

    #[tokio::test]
    async fn a_flat_answer_is_no_opinion_rather_than_low_risk() {
        let body = r#"{"choice":"no","confidence":0.5,"probabilities":[{"option":"no","probability":0.5,"logit":0.0},{"option":"yes","probability":0.5,"logit":0.0}],"latency_ms":0.0,"cached":false}"#;
        let provider = JevProvider::new(serving("200 OK", body));
        let error = provider.noul("rm -rf /", "destructive?").await.unwrap_err();
        assert!(error.to_string().contains("50/50"), "{error}");
    }

    #[tokio::test]
    async fn an_error_status_is_reported_for_the_cascade_to_catch() {
        let provider = JevProvider::new(serving("500 Internal Server Error", "{}"));
        let error = provider.noul("ls", "destructive?").await.unwrap_err();
        assert!(error.to_string().contains("500"), "{error}");
    }
}
