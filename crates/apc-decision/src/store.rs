//! Local model files: catalog, resumable download, checksum verification (SPEC.md §3.2).
//!
//! A model is downloaded to `<id>.bin.part`, resumed with a `Range` request, verified against the
//! catalog's SHA-256 and only then renamed into place (`.bin`). Nothing is ever bundled in the
//! binary.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use apc_core::{Error, Result};
use directories::ProjectDirs;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};

/// One downloadable decision model.
#[derive(Debug)]
pub struct ModelEntry {
    /// Identifier used on the command line and as the file name.
    pub id: &'static str,
    /// Where the artifact lives.
    pub url: &'static str,
    /// Lowercase hex SHA-256 of the complete artifact.
    pub sha256: &'static str,
    /// Exact size in bytes, so a short download is caught before hashing.
    pub bytes: u64,
}

/// Published models.
///
/// Empty on purpose: no decision model artifact exists yet (SPEC.md §8.11 — local-jev 2.0.0 ships
/// no weights), and inventing a URL here would make `apc model download` lie. Register a model by
/// adding one entry; the download, resume and verification code below is already ready for it.
pub const CATALOG: &[ModelEntry] = &[];

/// The catalog entry for `id`.
pub fn find(id: &str) -> Result<&'static ModelEntry> {
    CATALOG.iter().find(|entry| entry.id == id).ok_or_else(|| {
        Error::Model(format!(
            "unknown model `{id}`: no decision model has been published yet, and the heuristic \
             guard needs none"
        ))
    })
}

/// What is on disk for a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Neither the file nor a partial download exists.
    Missing,
    /// A download started and stopped; the value is how many bytes are already here.
    Partial(u64),
    /// The verified file is installed.
    Installed,
}

/// The `models/` directory of the user data directory.
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// `ProjectDirs` data dir, never a hardcoded path.
    pub fn new() -> Result<Self> {
        let dirs = ProjectDirs::from("dev", "aplix", "aplix").ok_or(Error::NoDataDir)?;
        Ok(Self {
            dir: dirs.data_dir().join("models"),
        })
    }

    /// A store rooted anywhere, for tests.
    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// The directory models live in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Path of an installed model.
    pub fn installed(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.bin"))
    }

    fn partial(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.bin.part"))
    }

    /// Whether the model is absent, half-downloaded or installed.
    pub fn status(&self, id: &str) -> Result<Status> {
        if self.installed(id).is_file() {
            return Ok(Status::Installed);
        }
        match std::fs::metadata(self.partial(id)) {
            Ok(meta) => Ok(Status::Partial(meta.len())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Status::Missing),
            Err(error) => Err(io_error(&self.partial(id), error)),
        }
    }

    /// Download `entry`, resuming whatever `.part` is already there, then verify and install it.
    ///
    /// `progress` receives the bytes written so far.
    pub async fn download<F>(&self, entry: &ModelEntry, mut progress: F) -> Result<PathBuf>
    where
        F: FnMut(u64),
    {
        let target = self.installed(entry.id);
        if target.is_file() {
            return Ok(target);
        }
        std::fs::create_dir_all(&self.dir).map_err(|error| io_error(&self.dir, error))?;

        let partial = self.partial(entry.id);
        let mut written = match std::fs::metadata(&partial) {
            Ok(meta) => meta.len(),
            Err(_) => 0,
        };
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&partial)
            .map_err(|error| io_error(&partial, error))?;

        let mut request = reqwest::Client::new().get(entry.url);
        if written > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={written}-"));
        }
        let response = request
            .send()
            .await
            .map_err(|error| Error::Provider(format!("{}: {error}", entry.url)))?;

        let status = response.status();
        if status == reqwest::StatusCode::OK && written > 0 {
            // The server ignored the range: start over rather than corrupt the file.
            file.set_len(0).map_err(|error| io_error(&partial, error))?;
            written = 0;
        } else if status != reqwest::StatusCode::PARTIAL_CONTENT
            && status != reqwest::StatusCode::OK
        {
            return Err(Error::Provider(format!(
                "{}: download failed with {status}",
                entry.url
            )));
        }

        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|error| Error::Provider(format!("{}: {error}", entry.url)))?;
            file.write_all(&chunk)
                .map_err(|error| io_error(&partial, error))?;
            written += chunk.len() as u64;
            progress(written);
        }
        file.flush().map_err(|error| io_error(&partial, error))?;

        if written != entry.bytes {
            return Err(Error::Model(format!(
                "{}: download ended after {written} of {} bytes; run the command again to resume",
                entry.id, entry.bytes
            )));
        }
        if sha256(&partial)? != entry.sha256.to_ascii_lowercase() {
            // A partial file that does not hash right cannot be resumed into a correct one.
            std::fs::remove_file(&partial).map_err(|error| io_error(&partial, error))?;
            return Err(Error::Model(format!(
                "{}: checksum mismatch, the download is corrupt and was discarded",
                entry.id
            )));
        }
        std::fs::rename(&partial, &target).map_err(|error| io_error(&target, error))?;
        Ok(target)
    }

    /// Recompute the checksum of an installed model.
    pub fn verify(&self, entry: &ModelEntry) -> Result<bool> {
        let path = self.installed(entry.id);
        if !path.is_file() {
            return Err(Error::Model(format!("{} is not installed", entry.id)));
        }
        Ok(sha256(&path)? == entry.sha256.to_ascii_lowercase())
    }

    /// Delete the model and any partial download. Returns whether anything was there.
    pub fn remove(&self, id: &str) -> Result<bool> {
        let mut removed = false;
        for path in [self.installed(id), self.partial(id)] {
            match std::fs::remove_file(&path) {
                Ok(()) => removed = true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error(&path, error)),
            }
        }
        Ok(removed)
    }
}

/// Lowercase hex SHA-256 of a file.
fn sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(|error| io_error(path, error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| io_error(path, error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
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
    use std::io::BufRead;
    use std::net::TcpListener;

    const BODY: &[u8] = b"0123456789abcdef";

    fn digest(body: &[u8]) -> String {
        format!("{:x}", Sha256::digest(body))
    }

    fn entry() -> ModelEntry {
        ModelEntry {
            id: "test-model",
            url: "",
            sha256: "",
            bytes: BODY.len() as u64,
        }
    }

    /// Serves `body` once; `honour_range` false simulates a server that ignores `Range`.
    fn serve(body: &'static [u8], honour_range: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(socket.try_clone().unwrap());
            let mut start = 0;
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                if let Some(rest) = line
                    .to_ascii_lowercase()
                    .strip_prefix("range: bytes=")
                    .map(|value| value.trim().trim_end_matches('-').to_owned())
                {
                    start = rest.parse().unwrap_or(0);
                }
                let done = line == "\r\n";
                line.clear();
                if done {
                    break;
                }
            }
            let start = if honour_range {
                start.min(body.len())
            } else {
                0
            };
            let slice = &body[start..];
            let header = if start > 0 {
                format!(
                    "HTTP/1.1 206 Partial Content\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    slice.len()
                )
            } else {
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    slice.len()
                )
            };
            socket.write_all(header.as_bytes()).unwrap();
            socket.write_all(slice).unwrap();
        });
        url
    }

    /// Download `body` from a one-shot server, returning the path and the first progress value.
    async fn downloaded(
        dir: &Path,
        body: &'static [u8],
        honour_range: bool,
    ) -> (Result<PathBuf>, u64) {
        let store = Store::at(dir.to_path_buf());
        let entry = ModelEntry {
            url: &*Box::leak(serve(body, honour_range).into_boxed_str()),
            sha256: &*Box::leak(digest(body).into_boxed_str()),
            ..entry()
        };
        let mut first = 0;
        let result = store
            .download(&entry, |written| {
                if first == 0 {
                    first = written;
                }
            })
            .await;
        (result, first)
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("apc-store-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[tokio::test]
    async fn a_complete_download_is_installed() {
        let dir = temp("complete");
        let path = downloaded(&dir, BODY, true).await.0.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), BODY);
        assert!(!dir.join("test-model.bin.part").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_partial_file_resumes_from_where_it_stopped() {
        let dir = temp("resume");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("test-model.bin.part"), &BODY[..5]).unwrap();
        let path = downloaded(&dir, BODY, true).await.0.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), BODY);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_corrupt_payload_is_never_installed() {
        let dir = temp("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::at(dir.clone());
        let entry = ModelEntry {
            url: &*Box::leak(serve(BODY, true).into_boxed_str()),
            sha256: "00",
            ..entry()
        };
        assert!(store.download(&entry, |_| {}).await.is_err());
        assert!(!dir.join("test-model.bin").exists());
        assert!(!dir.join("test-model.bin.part").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn verify_rejects_a_tampered_file() {
        let dir = temp("verify");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("test-model.bin"), b"tampered").unwrap();
        let store = Store::at(dir.clone());
        let entry = ModelEntry {
            sha256: &*Box::leak(digest(BODY).into_boxed_str()),
            ..entry()
        };
        assert!(!store.verify(&entry).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn remove_takes_the_model_and_the_partial_file() {
        let dir = temp("remove");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("test-model.bin"), BODY).unwrap();
        std::fs::write(dir.join("test-model.bin.part"), b"half").unwrap();
        let store = Store::at(dir.clone());
        assert_eq!(store.status("test-model").unwrap(), Status::Installed);
        assert!(store.remove("test-model").unwrap());
        assert_eq!(store.status("test-model").unwrap(), Status::Missing);
        assert!(!store.remove("test-model").unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_id_names_the_empty_catalog() {
        let error = find("open-jev").unwrap_err().to_string();
        assert!(error.contains("unknown model `open-jev`"), "{error}");
    }
}
