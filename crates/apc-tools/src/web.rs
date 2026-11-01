//! Web tools: a search through a local SearXNG instance and a plain page fetch.
//!
//! Neither needs a key, and neither is registered unless its input is configured: an
//! unconfigured tool must not show up in the schema the model is offered.

use apc_core::error::Error;
use apc_core::Result;
use apc_traits::Tool;
use serde_json::{json, Value};

/// Results returned when the model does not ask for a count.
const DEFAULT_RESULTS: usize = 5;
/// Upper bound, so one call cannot flood the context.
const MAX_RESULTS: usize = 10;

fn string(args: &Value, key: &str) -> Result<String> {
    args[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::Tool(format!("missing `{key}`")))
}

/// Search the web through a SearXNG instance.
pub struct WebSearch {
    base: String,
    client: reqwest::Client,
}

impl WebSearch {
    /// `base` is the instance root, e.g. `http://127.0.0.1:8888`.
    pub fn new(base: impl Into<String>) -> Self {
        apc_core::net::install_crypto_provider();
        Self {
            base: base.into().trim_end_matches('/').to_owned(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl Tool for WebSearch {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web through the local SearXNG instance and return the top results."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "What to search for." },
                "limit": {
                    "type": "integer",
                    "description": "How many results to return (1-10, default 5).",
                },
            },
            "required": ["query"],
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let query = string(&args, "query")?;
        let limit = args["limit"]
            .as_u64()
            .map_or(DEFAULT_RESULTS, |count| count as usize)
            .clamp(1, MAX_RESULTS);

        let response = self
            .client
            .get(format!("{}/search", self.base))
            .query(&[("q", query.as_str()), ("format", "json")])
            .send()
            .await
            .map_err(|error| Error::Tool(format!("searxng: {error}")))?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::Tool(format!("searxng answered {status}")));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|error| Error::Tool(format!("searxng: {error}")))?;

        let results = body["results"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        if results.is_empty() {
            return Ok("no results".to_owned());
        }

        let mut out = String::new();
        for (index, result) in results.iter().take(limit).enumerate() {
            let field = |key: &str| result[key].as_str().unwrap_or_default().trim();
            out.push_str(&format!(
                "{}. {}\n   {}\n",
                index + 1,
                field("title"),
                field("url")
            ));
            let snippet = field("content");
            if !snippet.is_empty() {
                out.push_str(&format!("   {snippet}\n"));
            }
        }
        Ok(out.trim_end().to_owned())
    }
}

/// Fetch one page over HTTP and hand the model its readable text.
pub struct FetchPage {
    max_bytes: usize,
    client: reqwest::Client,
}

impl FetchPage {
    /// Keep at most `max_bytes` of the page text.
    pub fn new(max_bytes: usize) -> Self {
        apc_core::net::install_crypto_provider();
        Self {
            max_bytes,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl Tool for FetchPage {
    fn name(&self) -> &str {
        "fetch_page"
    }

    fn description(&self) -> &str {
        "Fetch an http(s) page and return its readable text, truncated to a safe length."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Absolute http:// or https:// URL." },
            },
            "required": ["url"],
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let url = string(&args, "url")?;
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(Error::Tool(format!("`{url}` is not an http(s) URL")));
        }

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|error| Error::Tool(format!("{url}: {error}")))?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::Tool(format!("{url} answered {status}")));
        }
        let body = response
            .bytes()
            .await
            .map_err(|error| Error::Tool(format!("{url}: {error}")))?;

        Ok(truncate(
            &readable(&String::from_utf8_lossy(&body)),
            self.max_bytes,
        ))
    }
}

/// Strip markup, `<script>`/`<style>` bodies and the usual entities.
fn readable(html: &str) -> String {
    // Lowercased once: ASCII lowering keeps byte offsets, so it can drive the scan.
    let lower = html.to_ascii_lowercase();
    let mut text = String::with_capacity(html.len());
    let mut cursor = 0;
    while let Some(offset) = lower[cursor..].find('<') {
        text.push_str(&html[cursor..cursor + offset]);
        let start = cursor + offset;
        let Some(length) = lower[start..].find('>') else {
            break;
        };
        let tag = &lower[start + 1..start + length];
        cursor = start + length + 1;
        let skip = if tag.starts_with("script") {
            Some("script")
        } else if tag.starts_with("style") {
            Some("style")
        } else {
            None
        };
        if let Some(name) = skip {
            if let Some(close) = lower[cursor..].find(&format!("</{name}")) {
                cursor += close;
            }
        }
        text.push(' ');
    }
    text.push_str(&html[cursor.min(html.len())..]);

    let decoded = text
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cut `text` at `max` bytes on a char boundary, saying how much was dropped.
fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let cut = text
        .char_indices()
        .map(|(at, _)| at)
        .take_while(|at| *at <= max)
        .last()
        .unwrap_or(0);
    format!(
        "{}\n[truncated at {max} of {} bytes]",
        &text[..cut],
        text.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Answer one request with `body`, returning the server root.
    fn serve(body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let root = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut reader = socket.try_clone().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 1024];
            while !String::from_utf8_lossy(&request).contains("\r\n\r\n") {
                let read = reader.read(&mut chunk).unwrap_or(0);
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).unwrap();
        });
        root
    }

    #[tokio::test]
    async fn web_search_formats_the_results_it_was_asked_for() {
        let root = serve(
            r#"{"results":[{"title":"Rust","url":"https://rust-lang.org","content":"systems"},{"title":"Docs","url":"https://docs.rs","content":"reference"}]}"#,
        );
        let tool = WebSearch::new(root);

        let out = tool
            .execute(json!({ "query": "rust", "limit": 1 }))
            .await
            .expect("search answers");

        assert_eq!(out, "1. Rust\n   https://rust-lang.org\n   systems");
    }

    #[tokio::test]
    async fn web_search_says_when_nothing_came_back() {
        let root = serve(r#"{"results":[]}"#);
        let tool = WebSearch::new(root);

        let out = tool.execute(json!({ "query": "zzz" })).await.unwrap();

        assert_eq!(out, "no results");
    }

    #[tokio::test]
    async fn fetch_page_keeps_the_text_and_drops_the_markup() {
        let root = serve(
            "<html><head><style>body{color:red}</style><script>alert('x')</script></head><body><h1>Hello</h1><p>a &amp; b</p></body></html>",
        );
        let tool = FetchPage::new(65_536);

        let out = tool
            .execute(json!({ "url": format!("{root}/page") }))
            .await
            .unwrap();

        assert_eq!(out, "Hello a & b");
    }

    #[tokio::test]
    async fn fetch_page_truncates_a_page_too_big_for_the_context() {
        let body: &'static str = Box::leak("word ".repeat(1000).into_boxed_str());
        let root = serve(body);
        let tool = FetchPage::new(40);

        let out = tool
            .execute(json!({ "url": format!("{root}/page") }))
            .await
            .unwrap();

        assert!(out.starts_with("word word"), "{out}");
        assert!(out.ends_with(&format!("[truncated at 40 of {} bytes]", 4999)));
    }

    #[tokio::test]
    async fn fetch_page_refuses_a_non_http_url() {
        let tool = FetchPage::new(1024);

        let error = tool
            .execute(json!({ "url": "file:///etc/passwd" }))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("not an http(s) URL"), "{error}");
    }
}
