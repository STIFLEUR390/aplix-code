//! End-to-end checks for the shipped commands (no extra dev-dependencies).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("apc-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create sandbox");
    dir
}

/// Run `apc` in `dir` with a throwaway home, so the user layer never leaks in.
fn apc(dir: &Path, args: &[&str]) -> Output {
    apc_env(dir, args, &[])
}

fn apc_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    base_command(dir, args, env).output().expect("run apc")
}

/// Same, with `input` piped to stdin (for the REPL).
fn apc_stdin(dir: &Path, args: &[&str], env: &[(&str, &str)], input: &str) -> Output {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = base_command(dir, args, env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn apc");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("run apc")
}

fn base_command(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_apc"));
    command
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("xdg"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env_remove("APC_PROVIDER")
        .env_remove("APC_MODEL")
        .env_remove("APC_BASE_URL")
        .env_remove("APC_API_KEY")
        .env_remove("APC_PERMISSION_THRESHOLD")
        .env_remove("APC_TELEMETRY");
    for (key, value) in env {
        command.env(key, value);
    }
    command
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The table row whose first column is `key`.
fn row<'a>(text: &'a str, key: &str) -> &'a str {
    text.lines()
        .find(|line| line.split_whitespace().next() == Some(key))
        .unwrap_or_else(|| panic!("no `{key}` row in:\n{text}"))
}

#[test]
fn init_writes_a_config_and_refuses_to_overwrite() {
    let dir = sandbox("init");

    let first = apc(&dir, &["init"]);
    assert!(first.status.success(), "{}", stderr(&first));
    let text = std::fs::read_to_string(dir.join("apc.toml")).expect("apc.toml written");
    assert!(text.contains("[provider]"), "{text}");

    let second = apc(&dir, &["init"]);
    assert!(!second.status.success());
    assert!(
        stderr(&second).contains("already exists"),
        "{}",
        stderr(&second)
    );

    let forced = apc(&dir, &["init", "--force"]);
    assert!(forced.status.success(), "{}", stderr(&forced));
}

#[test]
fn config_reports_values_and_their_layer() {
    let dir = sandbox("layers");
    assert!(apc(&dir, &["init"]).status.success());

    let output = apc(&dir, &["config"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);

    assert!(text.contains("config files"), "{text}");
    assert!(row(&text, "provider.name").ends_with("project"));
    assert!(row(&text, "telemetry").ends_with("project"));
}

#[test]
fn local_file_env_and_flags_win_in_that_order() {
    let dir = sandbox("override");
    assert!(apc(&dir, &["init"]).status.success());
    std::fs::write(
        dir.join("apc.local.toml"),
        "[provider]\nmodel = \"local-model\"\n[decision]\nthreshold = 0.5\n",
    )
    .expect("write local file");

    let local = stdout(&apc(&dir, &["config"]));
    assert!(row(&local, "provider.model").contains("local-model"));
    assert!(row(&local, "decision.threshold").contains("0.5"));

    let env = stdout(&apc_env(&dir, &["config"], &[("APC_PROVIDER", "ollama")]));
    assert!(row(&env, "provider.name").contains("ollama"));
    assert!(row(&env, "provider.name").ends_with("env"));

    let flags = stdout(&apc(&dir, &["config", "--permission-threshold", "0.1"]));
    assert!(row(&flags, "decision.threshold").contains("0.1"));
    assert!(row(&flags, "decision.threshold").ends_with("flag"));
}

#[test]
fn env_telemetry_accepts_zero_and_one() {
    let dir = sandbox("telemetry");
    assert!(apc(&dir, &["init"]).status.success());

    let on = stdout(&apc_env(&dir, &["config"], &[("APC_TELEMETRY", "1")]));
    assert!(row(&on, "telemetry").contains("true"));

    let off = stdout(&apc_env(&dir, &["config"], &[("APC_TELEMETRY", "0")]));
    assert!(row(&off, "telemetry").contains("false"));
}

#[test]
fn env_names_the_decision_endpoint() {
    let dir = sandbox("endpoint");
    assert!(apc(&dir, &["init"]).status.success());

    let text = stdout(&apc_env(
        &dir,
        &["config"],
        &[("APC_DECISION_ENDPOINT", "http://127.0.0.1:8080")],
    ));
    assert!(row(&text, "decision.endpoint").contains("http://127.0.0.1:8080"));
}

#[test]
fn version_reports_the_decision_model() {
    let dir = sandbox("version");
    let output = apc(&dir, &["--version"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains(&format!("apc {}", env!("CARGO_PKG_VERSION"))),
        "{text}"
    );
    assert!(text.contains("decision model"), "{text}");
}

/// Serve one canned SSE body on 127.0.0.1, so `apc run` exercises the real HTTP path offline.
fn sse_server(body: &'static str) -> u16 {
    server("text/event-stream", body)
}

/// Serve one canned body with an arbitrary content type.
fn server(content_type: &'static str, body: &'static str) -> u16 {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a port");
    let port = listener.local_addr().expect("local address").port();
    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept one request");
        let mut request = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let read = socket.read(&mut chunk).expect("read the request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..read]);
            let text = String::from_utf8_lossy(&request).into_owned();
            let Some(end) = text.find("\r\n\r\n") else {
                continue;
            };
            let length = text[..end]
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if request.len() >= end + 4 + length {
                break;
            }
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\nconnection: close\r\n\r\n{body}"
        );
        socket.write_all(response.as_bytes()).expect("write body");
    });
    port
}

#[test]
fn run_streams_from_an_openai_compatible_endpoint() {
    let dir = sandbox("run");
    let port = sse_server(concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
        "data: [DONE]\n\n",
    ));
    let base = format!("http://127.0.0.1:{port}/v1");

    let output = apc_env(
        &dir,
        &["run", "say hello"],
        &[
            ("APC_PROVIDER", "openai"),
            ("APC_MODEL", "auto"),
            ("APC_BASE_URL", &base),
            ("APC_API_KEY", "test-key"),
        ],
    );

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "hello\n");
}

#[test]
fn run_without_a_key_names_the_variable() {
    let dir = sandbox("no-key");
    let output = apc_env(&dir, &["run", "hi"], &[("APC_PROVIDER", "openai")]);

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("OPENAI_API_KEY"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn run_reports_a_non_stream_response() {
    let dir = sandbox("not-sse");
    let port = server("text/html", "<html>gateway</html>");
    let base = format!("http://127.0.0.1:{port}/v1");

    let output = apc_env(
        &dir,
        &["run", "hi"],
        &[
            ("APC_PROVIDER", "openai"),
            ("APC_BASE_URL", &base),
            ("APC_API_KEY", "test-key"),
        ],
    );

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("expected an event stream"),
        "{}",
        stderr(&output)
    );
}

/// The single session file the sandbox holds.
fn session_file(dir: &Path) -> PathBuf {
    let sessions = dir.join("data/aplix/sessions");
    let files: Vec<PathBuf> = std::fs::read_dir(&sessions)
        .expect("sessions dir")
        .map(|entry| entry.expect("entry").path())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    files[0].clone()
}

#[test]
fn chat_persists_a_session_and_continues_it() {
    let dir = sandbox("chat");
    assert!(apc(&dir, &["init"]).status.success());
    let answer = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: [DONE]\n\n";

    let port = sse_server(answer);
    let base = format!("http://127.0.0.1:{port}/v1");
    let env = [("APC_BASE_URL", base.as_str()), ("APC_API_KEY", "test")];
    let first = apc_stdin(&dir, &["chat"], &env, "bonjour\nquit\n");
    assert!(first.status.success(), "{}", stderr(&first));

    let path = session_file(&dir);
    let text = std::fs::read_to_string(&path).expect("read session");
    assert_eq!(text.lines().count(), 2, "{text}");
    assert!(text.contains("\"role\":\"user\""), "{text}");
    assert!(text.contains("\"role\":\"assistant\""), "{text}");

    let port = sse_server(answer);
    let base = format!("http://127.0.0.1:{port}/v1");
    let env = [("APC_BASE_URL", base.as_str()), ("APC_API_KEY", "test")];
    let second = apc_stdin(&dir, &["chat", "--continue"], &env, "encore\nquit\n");
    assert!(second.status.success(), "{}", stderr(&second));
    assert!(
        stderr(&second).contains("continuing session"),
        "{}",
        stderr(&second)
    );

    let text = std::fs::read_to_string(session_file(&dir)).expect("read session");
    assert_eq!(text.lines().count(), 4, "{text}");
}

#[test]
fn model_list_says_nothing_is_published_yet() {
    let dir = sandbox("model-list");
    let out = apc(&dir, &["model", "list"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("no decision model is published yet"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn model_download_names_the_unknown_id() {
    let dir = sandbox("model-download");
    let out = apc(&dir, &["model", "download", "open-jev"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("unknown model `open-jev`"),
        "{}",
        stderr(&out)
    );
}
