//! `apc serve`: the same agent behind a loopback HTTP API (SPEC.md §7, v2.0).
//!
//! The server owns no logic of its own: it resolves the config the way every command does, builds
//! the agent with [`super::agent`], and persists through the same [`SqliteStore`] as `apc chat`.
//! Only the approver changes — a Question reaches the client over the event stream instead of
//! `/dev/tty`.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, Sse};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use directories::ProjectDirs;
use futures_util::stream::Stream;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::cors::CorsLayer;

use apc_core::chat::{ChatEvent, Message, SessionId};
use apc_core::config::Config;
use apc_memory::SqliteStore;
use apc_traits::{Approver, MemoryStore};

/// How long a destructive command waits for the client to answer.
const ASK_TIMEOUT: Duration = Duration::from_secs(120);

/// Everything a request needs. One instance per process.
struct Server {
    config: Config,
    store: SqliteStore,
    /// Confirmations waiting for `POST /v1/confirm`, keyed by the id sent to the client.
    pending: Mutex<HashMap<u64, mpsc::Sender<bool>>>,
    next: AtomicU64,
}

#[derive(Deserialize)]
struct RunRequest {
    prompt: String,
    /// Continue an existing session instead of starting one.
    session: Option<SessionId>,
    /// Provider for this run only; the resolved config stays the default.
    provider: Option<String>,
    /// Model for this run only.
    model: Option<String>,
}

#[derive(Deserialize)]
struct ConfirmRequest {
    id: u64,
    allow: bool,
}

#[derive(Serialize)]
struct SessionInfo {
    session: SessionId,
    messages: usize,
}

/// Serve until the process is stopped. Loopback only: there is no host option to widen it.
pub async fn serve(config: Config, port: u16) -> Result<()> {
    let store = SqliteStore::new()?;
    let token = read_token()?;
    let server = Arc::new(Server {
        config,
        store,
        pending: Mutex::new(HashMap::new()),
        next: AtomicU64::new(1),
    });

    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .await
        .with_context(|| format!("bind 127.0.0.1:{port}"))?;
    let address: SocketAddr = listener.local_addr()?;
    println!(
        "serving on http://{address} (token in {})",
        token_path()?.display()
    );

    axum::serve(listener, router(server, token)).await?;
    Ok(())
}

fn router(server: Arc<Server>, token: String) -> Router {
    Router::new()
        .route("/v1/run", post(run))
        .route("/v1/sessions", get(sessions))
        .route("/v1/config", get(config))
        .route("/v1/providers", get(providers))
        .route("/v1/confirm", post(confirm))
        .route_layer(middleware::from_fn_with_state(token, require_token))
        // Any origin: a webview is cross-origin, and the Bearer token is the real gate.
        .layer(CorsLayer::permissive())
        .with_state(server)
}

/// `Authorization: Bearer <token>`, nothing else.
async fn require_token(
    State(token): State<String>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let given = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if given == Some(token.as_str()) {
        return Ok(next.run(request).await);
    }
    Err(StatusCode::UNAUTHORIZED)
}

async fn config(State(server): State<Arc<Server>>) -> Json<Config> {
    Json(server.config.clone())
}

async fn providers() -> Json<&'static [&'static str]> {
    Json(apc_models::names())
}

async fn sessions(State(server): State<Arc<Server>>) -> Result<Json<Vec<SessionInfo>>, StatusCode> {
    let sessions = server
        .store
        .sessions()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        sessions
            .into_iter()
            .map(|(session, messages)| SessionInfo { session, messages })
            .collect(),
    ))
}

async fn confirm(
    State(server): State<Arc<Server>>,
    Json(body): Json<ConfirmRequest>,
) -> StatusCode {
    let sender = server.lock_pending().remove(&body.id);
    match sender {
        Some(sender) => {
            let _ = sender.send(body.allow);
            StatusCode::NO_CONTENT
        }
        None => StatusCode::NOT_FOUND,
    }
}

async fn run(
    State(server): State<Arc<Server>>,
    Json(body): Json<RunRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (events, receiver) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(server.run(body, events));
    let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (Ok(event), receiver))
    });
    Sse::new(stream)
}

/// The config this run uses: the server's, with the client's provider/model on top.
fn with_override(base: &Config, body: &RunRequest) -> Config {
    let mut config = base.clone();
    if let Some(provider) = &body.provider {
        config.use_provider(provider, body.model.as_deref());
    } else if let Some(model) = &body.model {
        config.provider.model = model.clone();
    }
    config
}

impl Server {
    fn lock_pending(&self) -> std::sync::MutexGuard<'_, HashMap<u64, mpsc::Sender<bool>>> {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// One turn, reported as events. Everything that can fail is an event: the stream has already
    /// started, so an error cannot be a status code.
    async fn run(
        self: Arc<Self>,
        body: RunRequest,
        events: tokio::sync::mpsc::UnboundedSender<Event>,
    ) {
        let run_config = with_override(&self.config, &body);
        let session = body.session.unwrap_or_else(|| self.store.new_session());
        let mut messages = match self.store.load(Some(&session)).await {
            Ok(messages) => messages,
            Err(error) => return report(&events, &error.to_string()),
        };

        let root = match std::env::current_dir() {
            Ok(root) => root,
            Err(error) => return report(&events, &error.to_string()),
        };
        super::apply_skill(&mut messages, &root, &body.prompt);
        super::apply_memory(&self.store, &mut messages, &session, &body.prompt).await;

        // Persist the prompt before the provider is called, so a crash keeps it (as `apc chat`).
        let question = Message::user(body.prompt);
        if let Err(error) = self.store.append(&session, &question).await {
            return report(&events, &error.to_string());
        }
        messages.push(question);

        let approver = Arc::new(ApiApprover {
            server: Arc::clone(&self),
            events: events.clone(),
        });
        let agent = match super::agent(&run_config, approver, apc_core::config::Mode::Build) {
            Ok(agent) => agent,
            Err(error) => return report(&events, &error.to_string()),
        };

        let gate = events.clone();
        let before = messages.len();
        let result = agent
            .run(&mut messages, |event| {
                if let ChatEvent::Delta(delta) = event {
                    let _ = gate.send(Event::default().data(json!({ "delta": delta }).to_string()));
                }
            })
            .await;
        if let Err(error) = result {
            return report(&events, &error.to_string());
        }

        for message in &messages[before..] {
            if let Err(error) = self.store.append(&session, message).await {
                return report(&events, &error.to_string());
            }
        }
        let _ = events
            .send(Event::default().data(json!({ "done": true, "session": session }).to_string()));
    }
}

fn report(events: &tokio::sync::mpsc::UnboundedSender<Event>, error: &str) {
    let _ = events.send(Event::default().data(json!({ "error": error }).to_string()));
}

/// Answers a destructive-command question through the API instead of the terminal.
struct ApiApprover {
    server: Arc<Server>,
    events: tokio::sync::mpsc::UnboundedSender<Event>,
}

impl Approver for ApiApprover {
    fn confirm(&self, command: &str, risk: f64) -> apc_core::Result<bool> {
        let id = self.server.next.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel();
        self.server.lock_pending().insert(id, sender);
        let ask = json!({ "ask": { "id": id, "command": command, "risk": risk } });
        let _ = self.events.send(Event::default().data(ask.to_string()));
        // shortcut: `Approver::confirm` is sync, so this blocks a runtime thread until the answer
        // or the timeout; one thread per pending question. Revisit if concurrent runs matter.
        let allowed = receiver.recv_timeout(ASK_TIMEOUT).unwrap_or(false);
        self.server.lock_pending().remove(&id);
        Ok(allowed)
    }
}

fn token_path() -> Result<PathBuf> {
    let dirs =
        ProjectDirs::from("dev", "aplix", "aplix").context("no data directory on this machine")?;
    Ok(dirs.data_dir().join("serve.token"))
}

/// The token in `data_dir/serve.token`, generated on first use.
fn read_token() -> Result<String> {
    let path = token_path()?;
    if let Ok(text) = std::fs::read_to_string(&path) {
        let token = text.trim();
        if token.len() >= 32 {
            return Ok(token.to_owned());
        }
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let token = new_token();
    std::fs::write(&path, &token).with_context(|| format!("write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(token)
}

/// 256 bits of hex from std's randomly seeded hasher: no dependency for one string.
fn new_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let mut token = String::new();
    for round in 0..4u8 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u8(round);
        token.push_str(&format!("{:016x}", hasher.finish()));
    }
    token
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Arc<Server> {
        let dir = std::env::temp_dir().join(format!("apc-serve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Arc::new(Server {
            config: Config::default(),
            store: SqliteStore::open_at(dir.join("memory.db")).expect("store"),
            pending: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
        })
    }

    /// A destructive command is refused when nobody answers, and allowed when the API answers.
    #[test]
    fn the_guard_answers_through_the_api() {
        let server = server();
        let (events, mut asked) = tokio::sync::mpsc::unbounded_channel();
        let approver = ApiApprover {
            server: Arc::clone(&server),
            events,
        };
        let answered = std::thread::spawn(move || approver.confirm("rm -rf /", 0.99));

        assert!(
            asked.blocking_recv().is_some(),
            "the question reached the stream"
        );
        // Answer it the way `POST /v1/confirm` does.
        let sender = server.lock_pending().remove(&1).expect("pending question");
        sender.send(true).expect("answer");
        assert!(answered.join().expect("thread").expect("confirm"));
    }
}
