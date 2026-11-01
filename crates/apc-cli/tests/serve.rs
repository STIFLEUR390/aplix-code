//! `apc serve` end to end: a real process, a real socket, and the same stubs as `cli.rs`.
//!
//! The client is a hand-rolled `TcpStream` request because the test binary may not add a
//! dev-dependency (see the note at the top of `cli.rs`).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The port the server was told to use, plus a token that is *not* the server's.
struct Sandbox {
    dir: PathBuf,
    port: u16,
    child: Child,
}

impl Sandbox {
    fn token(&self) -> String {
        std::fs::read_to_string(self.dir.join("data/aplix/serve.token"))
            .expect("serve.token")
            .trim()
            .to_owned()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// An SSE stub: answers one `POST /chat/completions` with `content`, then forgets it.
fn sse_server(content: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
        let mut length = 0;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap_or(0);
            }
            if line == "\r\n" {
                break;
            }
        }
        let mut body = vec![0; length];
        let _ = reader.read_exact(&mut body);

        let answer = format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{content}\"}}}}]}}\n\ndata: [DONE]\n\n"
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
            answer.len()
        );
        let _ = stream.write_all(response.as_bytes());
    });
    port
}

/// A port nothing listens on, for the server under test.
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().expect("addr").port()
}

/// Start `apc serve` against the stub and wait until the token file appears.
fn serve(dir: &Path, provider_port: u16) -> Sandbox {
    let port = free_port();
    let mut command = Command::new(env!("CARGO_BIN_EXE_apc"));
    command
        .args(["serve", "--port", &port.to_string()])
        .current_dir(dir)
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("xdg"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("APC_PROVIDER", "openai")
        .env("APC_MODEL", "stub")
        .env("APC_BASE_URL", format!("http://127.0.0.1:{provider_port}"))
        .env("APC_API_KEY", "test")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for key in [
        "APC_PERMISSION_THRESHOLD",
        "APC_DECISION_ENDPOINT",
        "APC_TELEMETRY",
        "APC_SEARXNG_URL",
        "APC_CONFIG",
    ] {
        command.env_remove(key);
    }
    let child = command.spawn().expect("spawn apc serve");

    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if dir.join("data/aplix/serve.token").is_file() {
            return Sandbox {
                dir: dir.to_path_buf(),
                port,
                child,
            };
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut child = child;
    let _ = child.kill();
    let _ = child.wait();
    panic!("apc serve never wrote its token");
}

struct HttpResponse {
    status: u16,
    body: String,
}

/// One HTTP request, `Connection: close`, so the body is everything left on the socket.
fn request(
    port: u16,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> HttpResponse {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let body = body.unwrap_or("");
    let auth = token.map_or(String::new(), |token| {
        format!("authorization: Bearer {token}\r\n")
    });
    let head = format!(
        "{method} {path} HTTP/1.1\r\nhost: 127.0.0.1\r\n{auth}content-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(head.as_bytes()).expect("write");

    let mut raw = String::new();
    stream.read_to_string(&mut raw).expect("read");
    let status = raw
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("status line");
    let body = raw
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or("");
    HttpResponse {
        status,
        body: body.to_owned(),
    }
}

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("apc-serve-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("sandbox");
    dir
}

#[test]
fn a_request_without_the_token_is_refused() {
    let dir = sandbox("token");
    let server = serve(&dir, sse_server("unused"));

    assert_eq!(
        request(server.port, "GET", "/v1/sessions", None, None).status,
        401
    );
    assert_eq!(
        request(server.port, "GET", "/v1/sessions", Some("wrong"), None).status,
        401
    );
    assert_eq!(
        request(server.port, "GET", "/v1/config", None, None).status,
        401
    );
    assert_eq!(
        request(
            server.port,
            "GET",
            "/v1/config",
            Some(&server.token()),
            None
        )
        .status,
        200
    );
    // The token is 32 bytes of randomness, never a constant.
    assert_eq!(server.token().len(), 64);
}

#[test]
fn run_streams_sse_and_persists_the_session() {
    let dir = sandbox("run");
    let server = serve(&dir, sse_server("bonjour"));

    let prompt = r#"{"prompt":"salut"}"#;
    let response = request(
        server.port,
        "POST",
        "/v1/run",
        Some(&server.token()),
        Some(prompt),
    );
    assert_eq!(response.status, 200, "{}", response.body);
    assert!(
        response.body.contains(r#""delta":"bonjour""#),
        "{}",
        response.body
    );
    assert!(
        response.body.contains(r#""done":true"#),
        "{}",
        response.body
    );

    let sessions = request(
        server.port,
        "GET",
        "/v1/sessions",
        Some(&server.token()),
        None,
    );
    assert_eq!(sessions.status, 200);
    assert!(
        sessions.body.contains(r#""messages":2"#),
        "{}",
        sessions.body
    );
}

#[test]
fn config_reports_the_effective_values() {
    let dir = sandbox("config");
    let server = serve(&dir, sse_server("unused"));

    let response = request(
        server.port,
        "GET",
        "/v1/config",
        Some(&server.token()),
        None,
    );
    assert_eq!(response.status, 200);
    assert!(
        response.body.contains(r#""model":"stub""#),
        "{}",
        response.body
    );
}
