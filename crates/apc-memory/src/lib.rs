#![forbid(unsafe_code)]
//! Session memory.
//!
//! One JSONL file per session under the user data directory (SPEC.md §2.5): a message per line, so
//! a conversation can be replayed and a torn write costs one line instead of the file.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use apc_core::chat::{Message, SessionId};
use apc_core::{Error, Result};
use apc_traits::MemoryStore;
use directories::ProjectDirs;

/// Session history in `<data dir>/aplix/sessions/<id>.jsonl`.
pub struct JsonlStore {
    dir: PathBuf,
}

impl JsonlStore {
    /// The store under the platform's user data directory.
    pub fn new() -> Result<Self> {
        let dirs = ProjectDirs::from("dev", "aplix", "aplix").ok_or(Error::NoDataDir)?;
        Ok(Self::at(dirs.data_dir().join("sessions")))
    }

    /// A store rooted at `dir` (tests, or an explicit location).
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Where session files live.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// A fresh session id. The leading seconds keep ids ordered like the files they name.
    pub fn new_session(&self) -> SessionId {
        let micros = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_micros());
        SessionId::new(format!(
            "{:013}-{:06}",
            micros / 1_000_000,
            micros % 1_000_000
        ))
    }

    /// The most recent session on disk, if any.
    pub fn latest_session(&self) -> Result<Option<SessionId>> {
        Ok(self
            .session_files()?
            .pop()
            .map(|path| SessionId::new(path.file_stem().unwrap_or_default().to_string_lossy())))
    }

    fn path(&self, session: &SessionId) -> PathBuf {
        self.dir.join(format!("{}.jsonl", session.as_str()))
    }

    /// Session files, oldest first (ids sort chronologically).
    fn session_files(&self) -> Result<Vec<PathBuf>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(io_error(&self.dir, source)),
        };
        let mut paths = Vec::new();
        for entry in entries {
            let path = entry.map_err(|source| io_error(&self.dir, source))?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            {
                paths.push(path);
            }
        }
        paths.sort();
        Ok(paths)
    }

    fn append_line(&self, session: &SessionId, message: &Message) -> Result<()> {
        let path = self.path(session);
        fs::create_dir_all(&self.dir).map_err(|source| io_error(&self.dir, source))?;
        let mut line = serde_json::to_string(message).map_err(|source| Error::Json {
            path: path.display().to_string(),
            source,
        })?;
        line.push('\n');
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| file.write_all(line.as_bytes()))
            .map_err(|source| io_error(&path, source))
    }

    fn read(&self, session: &SessionId) -> Result<Vec<Message>> {
        let path = self.path(session);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(io_error(&path, source)),
        };
        Ok(text
            .lines()
            .filter(|line| !line.trim().is_empty())
            // An interrupted append leaves a fragment: dropping it beats losing the session.
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect())
    }
}

#[async_trait::async_trait]
impl MemoryStore for JsonlStore {
    async fn append(&self, session: &SessionId, message: &Message) -> Result<()> {
        self.append_line(session, message)
    }

    async fn load(&self, session: Option<&SessionId>) -> Result<Vec<Message>> {
        match session {
            Some(session) => self.read(session),
            None => match self.latest_session()? {
                Some(session) => self.read(&session),
                None => Ok(Vec::new()),
            },
        }
    }
}

fn io_error(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.display().to_string(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> JsonlStore {
        let dir = std::env::temp_dir().join(format!("apc-memory-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        JsonlStore::at(dir)
    }

    #[tokio::test]
    async fn append_then_load_round_trips() {
        let store = store("roundtrip");
        let session = store.new_session();
        store
            .append(&session, &Message::user("bonjour"))
            .await
            .expect("append");
        store
            .append(&session, &Message::assistant("salut"))
            .await
            .expect("append");

        let loaded = store.load(Some(&session)).await.expect("load");
        assert_eq!(
            loaded,
            vec![Message::user("bonjour"), Message::assistant("salut")]
        );
    }

    #[tokio::test]
    async fn the_latest_session_is_the_one_loaded_without_an_id() {
        let store = store("latest");
        let older = SessionId::new("0000000000001-000000");
        let newer = SessionId::new("0000000000002-000000");
        store
            .append(&older, &Message::user("vieux"))
            .await
            .expect("append");
        store
            .append(&newer, &Message::user("recent"))
            .await
            .expect("append");

        assert_eq!(store.latest_session().expect("latest"), Some(newer));
        assert_eq!(
            store.load(None).await.expect("load"),
            vec![Message::user("recent")]
        );
    }

    #[tokio::test]
    async fn a_torn_last_line_does_not_break_loading() {
        let store = store("torn");
        let session = store.new_session();
        store
            .append(&session, &Message::user("bonjour"))
            .await
            .expect("append");
        let path = store.dir().join(format!("{session}.jsonl"));
        let mut file = OpenOptions::new().append(true).open(&path).expect("open");
        file.write_all(b"{\"role\":\"assistant\",\"cont")
            .expect("write fragment");

        let loaded = store.load(Some(&session)).await.expect("load");
        assert_eq!(loaded, vec![Message::user("bonjour")]);
    }

    #[tokio::test]
    async fn an_unknown_session_loads_empty() {
        let store = store("missing");
        assert!(store
            .load(Some(&SessionId::new("nope")))
            .await
            .expect("load")
            .is_empty());
    }

    #[test]
    fn new_uses_the_platform_data_dir() {
        if let Ok(store) = JsonlStore::new() {
            assert!(store.dir().ends_with("aplix/sessions"));
        }
    }
}
