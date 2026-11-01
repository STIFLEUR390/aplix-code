//! SQLite session memory with an FTS5 index (v1.3).
//!
//! One file, `<data dir>/memory.db`: `messages` holds the JSON body of every message exactly as the
//! JSONL format wrote it, and `messages_fts` indexes that text so a prompt can be answered from
//! what earlier sessions said. No network, no embeddings: FTS5 ranking is the relevance.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use apc_core::chat::{Message, SessionId};
use apc_core::{Error, Result};
use apc_traits::{MemoryStore, Recall};
use directories::ProjectDirs;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use crate::jsonl::{session_files, JsonlStore};
use crate::{io_error, new_session_id, words};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY);
CREATE TABLE IF NOT EXISTS messages (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    session TEXT NOT NULL,
    body    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_of_session ON messages (session, id);
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts
    USING fts5(body, content='messages', content_rowid='id');
CREATE TRIGGER IF NOT EXISTS messages_indexed AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts (rowid, body) VALUES (new.id, new.body);
END;
"#;

/// Session history in `<data dir>/memory.db`.
pub struct SqliteStore {
    path: PathBuf,
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// The store under the platform's user data directory, with any JSONL session directory
    /// imported first.
    pub fn new() -> Result<Self> {
        let dirs = ProjectDirs::from("dev", "aplix", "aplix").ok_or(Error::NoDataDir)?;
        let store = Self::open_at(dirs.data_dir().join("memory.db"))?;
        store.import_jsonl(JsonlStore::new()?.dir())?;
        Ok(store)
    }

    /// A store at `path` (tests, or an explicit location).
    pub fn open_at(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
        let conn =
            Connection::open_with_flags(&path, flags).map_err(|source| db_error(&path, source))?;
        conn.execute_batch(SCHEMA)
            .map_err(|source| db_error(&path, source))?;
        Ok(Self {
            path,
            conn: Mutex::new(conn),
        })
    }

    /// Where the database lives.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A fresh session id.
    pub fn new_session(&self) -> SessionId {
        new_session_id()
    }

    /// The most recent session, if any. Ids sort chronologically.
    pub fn latest_session(&self) -> Result<Option<SessionId>> {
        let conn = self.conn();
        conn.query_row(
            "SELECT id FROM sessions ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map(|id| id.map(SessionId::new))
        .map_err(|source| db_error(&self.path, source))
    }

    /// Every session with its message count, newest first. Empty sessions are left out.
    pub fn sessions(&self) -> Result<Vec<(SessionId, usize)>> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT session, COUNT(*) FROM messages GROUP BY session ORDER BY session DESC",
            )
            .map_err(|source| db_error(&self.path, source))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    SessionId::new(row.get::<_, String>(0)?),
                    row.get::<_, i64>(1)? as usize,
                ))
            })
            .map_err(|source| db_error(&self.path, source))?;
        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row.map_err(|source| db_error(&self.path, source))?);
        }
        Ok(sessions)
    }

    /// Imports every JSONL session file the database does not know yet, and returns how many.
    ///
    /// Idempotent: a session already stored is skipped, so re-running costs one query per file. A
    /// torn last line is dropped, exactly as loading the file would.
    pub fn import_jsonl(&self, dir: &Path) -> Result<usize> {
        let mut imported = 0;
        for path in session_files(dir)? {
            let session = SessionId::new(path.file_stem().unwrap_or_default().to_string_lossy());
            if self.has_session(&session)? {
                continue;
            }
            let text = fs::read_to_string(&path).map_err(|source| io_error(&path, source))?;
            let messages: Vec<Message> = text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect();
            for message in &messages {
                self.insert(&session, message)?;
            }
            // An empty or fully torn file still counts as imported: a rerun must not rescan it.
            self.insert_session(&session)?;
            imported += 1;
        }
        Ok(imported)
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn has_session(&self, session: &SessionId) -> Result<bool> {
        let conn = self.conn();
        conn.query_row(
            "SELECT 1 FROM sessions WHERE id = ?1",
            [session.as_str()],
            |_| Ok(()),
        )
        .optional()
        .map(|found| found.is_some())
        .map_err(|source| db_error(&self.path, source))
    }

    fn insert_session(&self, session: &SessionId) -> Result<()> {
        self.conn()
            .execute(
                "INSERT OR IGNORE INTO sessions (id) VALUES (?1)",
                [session.as_str()],
            )
            .map(|_| ())
            .map_err(|source| db_error(&self.path, source))
    }

    fn insert(&self, session: &SessionId, message: &Message) -> Result<()> {
        let body = serde_json::to_string(message).map_err(|source| Error::Json {
            path: self.path.display().to_string(),
            source,
        })?;
        self.insert_session(session)?;
        self.conn()
            .execute(
                "INSERT INTO messages (session, body) VALUES (?1, ?2)",
                params![session.as_str(), body],
            )
            .map(|_| ())
            .map_err(|source| db_error(&self.path, source))
    }

    fn read(&self, session: &SessionId) -> Result<Vec<Message>> {
        let conn = self.conn();
        let mut statement = conn
            .prepare("SELECT body FROM messages WHERE session = ?1 ORDER BY id")
            .map_err(|source| db_error(&self.path, source))?;
        let rows = statement
            .query_map([session.as_str()], |row| row.get::<_, String>(0))
            .map_err(|source| db_error(&self.path, source))?;
        let mut messages = Vec::new();
        for row in rows {
            let body = row.map_err(|source| db_error(&self.path, source))?;
            let message = serde_json::from_str(&body).map_err(|source| Error::Json {
                path: self.path.display().to_string(),
                source,
            })?;
            messages.push(message);
        }
        Ok(messages)
    }

    fn search_sessions(&self, query: &str, limit: usize) -> Result<Vec<Recall>> {
        let words = words(query);
        if words.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        // Quoting each word keeps a prompt's punctuation from being read as match syntax.
        let expression = words
            .iter()
            .map(|word| format!("\"{word}\""))
            .collect::<Vec<_>>()
            .join(" OR ");
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT m.session, snippet(messages_fts, 0, '[', ']', '…', 16)
                 FROM messages_fts JOIN messages m ON m.id = messages_fts.rowid
                 WHERE messages_fts MATCH ?1
                 ORDER BY messages_fts.rank
                 LIMIT ?2",
            )
            .map_err(|source| db_error(&self.path, source))?;
        let rows = statement
            .query_map(params![expression, limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|source| db_error(&self.path, source))?;
        let mut recalls = Vec::new();
        for row in rows {
            let (session, snippet) = row.map_err(|source| db_error(&self.path, source))?;
            recalls.push(Recall {
                session: SessionId::new(session),
                snippet,
            });
        }
        Ok(recalls)
    }
}

#[async_trait::async_trait]
impl MemoryStore for SqliteStore {
    async fn append(&self, session: &SessionId, message: &Message) -> Result<()> {
        self.insert(session, message)
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

    /// Best FTS5 match first. A hit is one message, so a session can fill the whole limit.
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Recall>> {
        self.search_sessions(query, limit)
    }
}

fn db_error(path: &Path, source: rusqlite::Error) -> Error {
    Error::Memory(format!("{}: {source}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("apc-sqlite-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("memory.db")
    }

    #[tokio::test]
    async fn append_then_load_round_trips() {
        let store = SqliteStore::open_at(path("roundtrip")).expect("open");
        let session = store.new_session();
        store
            .append(&session, &Message::user("bonjour"))
            .await
            .expect("append");
        store
            .append(&session, &Message::assistant("salut"))
            .await
            .expect("append");

        assert_eq!(
            store.load(Some(&session)).await.expect("load"),
            vec![Message::user("bonjour"), Message::assistant("salut")]
        );
        assert_eq!(
            store.latest_session().expect("latest"),
            Some(session.clone())
        );
        assert_eq!(store.sessions().expect("sessions"), vec![(session, 2)]);
    }

    #[tokio::test]
    async fn search_finds_a_session_by_keyword() {
        let store = SqliteStore::open_at(path("search")).expect("open");
        let wanted = SessionId::new("0000000000001-000000");
        store
            .append(
                &wanted,
                &Message::user("how are rustls providers configured?"),
            )
            .await
            .expect("append");
        store
            .append(
                &SessionId::new("0000000000002-000000"),
                &Message::user("something else entirely"),
            )
            .await
            .expect("append");

        let recalls = store.search("rustls provider", 5).await.expect("search");
        assert_eq!(recalls.len(), 1, "{recalls:?}");
        assert_eq!(recalls[0].session, wanted);
        assert!(recalls[0].snippet.contains("[rustls]"), "{recalls:?}");
    }

    #[tokio::test]
    async fn migration_imports_jsonl_and_runs_twice_without_duplicating() {
        let dir = std::env::temp_dir().join(format!("apc-jsonl-{}-migrate", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let legacy = JsonlStore::at(&dir);
        let older = SessionId::new("0000000000001-000000");
        let newer = SessionId::new("0000000000002-000000");
        legacy
            .append(&older, &Message::user("ancienne session"))
            .await
            .expect("append");
        legacy
            .append(&newer, &Message::user("récente"))
            .await
            .expect("append");
        // v0.2b's torn tail: the fragment must be dropped, not the session.
        let torn = dir.join(format!("{newer}.jsonl"));
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&torn)
            .expect("open");
        file.write_all(b"{\"role\":\"assistant\",\"cont")
            .expect("write fragment");

        let store = SqliteStore::open_at(path("migrate")).expect("open");
        assert_eq!(store.import_jsonl(&dir).expect("import"), 2);
        assert_eq!(
            store.load(Some(&older)).await.expect("load"),
            vec![Message::user("ancienne session")]
        );
        assert_eq!(
            store.load(Some(&newer)).await.expect("load"),
            vec![Message::user("récente")]
        );

        assert_eq!(store.import_jsonl(&dir).expect("re-import"), 0);
        assert_eq!(
            store.load(Some(&newer)).await.expect("load"),
            vec![Message::user("récente")]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_read_only_database_reports_a_clear_error() {
        use std::os::unix::fs::PermissionsExt;

        let path = path("readonly");
        let store = SqliteStore::open_at(&path).expect("open");
        let session = store.new_session();
        store
            .append(&session, &Message::user("écrit une fois"))
            .await
            .expect("append");
        drop(store);
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o444);
        fs::set_permissions(&path, permissions).expect("chmod");

        let message = match SqliteStore::open_at(&path) {
            Err(error) => error.to_string(),
            Ok(store) => store
                .append(&session, &Message::user("impossible"))
                .await
                .expect_err("a read-only database must refuse a write")
                .to_string(),
        };
        assert!(message.contains("memory.db"), "{message}");
    }

    #[tokio::test]
    async fn an_unknown_session_loads_empty() {
        let store = SqliteStore::open_at(path("missing")).expect("open");
        assert!(store
            .load(Some(&SessionId::new("nope")))
            .await
            .expect("load")
            .is_empty());
        assert_eq!(store.latest_session().expect("latest"), None);
    }
}
