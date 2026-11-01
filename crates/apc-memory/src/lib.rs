#![forbid(unsafe_code)]
//! Session memory.
//!
//! Sessions live in SQLite, full-text indexed with FTS5 (`SqliteStore`, v1.3). The one JSONL file
//! per session of v0.2b (`JsonlStore`) stays as the import/export format, and an existing
//! `sessions/` directory is imported on first open (SPEC.md §2.5).

mod jsonl;
mod sqlite;

pub use jsonl::JsonlStore;
pub use sqlite::SqliteStore;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use apc_core::chat::SessionId;
use apc_core::Error;

/// A fresh session id. The leading seconds keep ids ordered like the sessions they name.
pub fn new_session_id() -> SessionId {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_micros());
    SessionId::new(format!(
        "{:013}-{:06}",
        micros / 1_000_000,
        micros % 1_000_000
    ))
}

/// The words of `text` a full-text search can key on: lowercased, three letters or more, unique.
///
/// A prompt is prose, not a query syntax, so every backend gets words instead of a match
/// expression.
pub(crate) fn words(text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for word in text.split(|c: char| !c.is_alphanumeric()) {
        if word.chars().count() < 3 {
            continue;
        }
        let word = word.to_lowercase();
        if !words.contains(&word) {
            words.push(word);
        }
    }
    words
}

/// One line of a message, cut on a character boundary.
pub(crate) fn snippet(text: &str) -> String {
    const MAX: usize = 160;
    let text = text.trim();
    match text.char_indices().nth(MAX) {
        Some((index, _)) => format!("{}…", &text[..index]),
        None => text.to_owned(),
    }
}

pub(crate) fn io_error(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.display().to_string(),
        source,
    }
}
