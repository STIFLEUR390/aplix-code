# Commands

v0.4 is implemented; commands marked *target* do not exist yet.

## Build / run

```bash
cargo build --release
./target/release/apc --version        # also prints the loaded decision model
./target/release/apc init             # writes ./apc.toml
./target/release/apc config           # effective config + the layer behind each key
cargo run -p apc-cli -- config --model llama3   # flag layer override
```

```bash
./target/debug/apc run "list the crates"        # one-shot agent turn
./target/debug/apc chat                          # REPL, persists to the session file
./target/debug/apc chat --continue               # replays the latest session
cargo build --offline --features open-jev        # add the local-jev decision client
APC_DECISION_ENDPOINT=http://127.0.0.1:8080 ./target/debug/apc run "…"
```

`apc model list|status|verify|remove <id>` work today; `download` reports an unknown model because
the catalog is empty (SPEC §8.12).

Requires Rust 1.80+. Only system dependency expected: OpenSSL, or `rustls` if the feature is enabled.

## Verify (in this order before claiming done)

```bash
cargo fmt --all --check
cargo clippy --offline --all-targets -- -D warnings
cargo test --offline --workspace
cargo test --offline -p apc-decision --features open-jev   # the open-jev build is a separate path
cargo build --offline --no-default-features               # the shipped shape (no Sentry)
```


```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
ls -lh target/release/apc             # must stay under 15 MB (7.38 MB with Sentry, 5.64 MB with
                                      # --no-default-features; reqwest+tokio dominate, not Sentry)
```

## CI and releases

```bash
# exactly what .github/workflows/ci.yml runs (three feature shapes)
for f in "" "--features open-jev" "--features telemetry"; do
  cargo clippy --offline --all-targets $f -- -D warnings && cargo test --offline --workspace $f
done

# the size gate, locally
size=$(wc -c < target/release/apc); test "$size" -lt 15728640 && echo "ok: $size"

# publish: a tag builds Linux/macOS/Windows and creates the GitHub release
git tag v1.0.0 && git push origin v1.0.0
```

Release artifacts are built with `--no-default-features`, so no Sentry code ships. The local
`dist/` folder from a dry run of the packaging step is git-ignored.

## Single test / single crate

```bash
cargo test -p apc-core config              # one crate, tests matching "config"
cargo test -p apc-cli --test cli           # CLI end-to-end
cargo test -p apc-tools                    # path confinement + refused confirmation
cargo test -p apc-decision                 # guard patterns, true and false positives
cargo test precedence -- --nocapture       # one test + stdout
```

`crates/apc-cli/tests/cli.rs` spawns `CARGO_BIN_EXE_apc` with a sandbox `HOME`/`XDG_CONFIG_HOME` per
test (`std::env::temp_dir()/apc-cli-<pid>-<name>`), so it needs no extra dev-dependency and never
reads the developer's real config.

## Workspace shape (v0.3)

Root `Cargo.toml`: `members = ["crates/*"]`, `resolver = "2"`, `[workspace.package]` (version, edition
2021, `rust-version = "1.80"`, MIT), `[workspace.dependencies]` (`apc-agent`, `apc-core`, `apc-decision`, `apc-memory`, `apc-models`,
`apc-plugins`, `apc-tools`, `apc-traits`, `anyhow`, `async-trait`, `bytes`,
`clap`, `directories`, `futures-core`, `futures-util`, `reqwest`, `sentry`, `serde`, `serde_json`,
`thiserror`, `tokio`, `toml` — add entries only when a crate uses them), plus the
release profile:

```toml
[profile.release]
lto = true
codegen-units = 1
strip = "symbols"
panic = "abort"
opt-level = 3
```

Config paths: user `~/.config/aplix/apc.toml` (`ProjectDirs::from("dev", "aplix", "aplix")`), project
`./apc.toml`, local `./apc.local.toml`, env `APC_PROVIDER`/`APC_MODEL`/`APC_BASE_URL`/
`APC_PERMISSION_THRESHOLD`/`APC_TELEMETRY`, flags `--provider`/`--model`/`--permission-threshold`.

`.env` at the repo root holds the local test values (git-ignored): `APC_PROVIDER=openai`,
`APC_BASE_URL=http://127.0.0.1:31415/v1`, `APC_API_KEY=…`, `APC_MODEL=auto`, `APC_SENTRY_DSN=…`,
`APC_TELEMETRY=1`. **Debug builds load it themselves** (`load_dotenv` in `crates/apc-cli/src/main.rs`);
release builds and `cargo test` do not. The trailing `/v1` matters: the adapter appends
`/chat/completions`, so a bare origin returns the relay's HTML page.

```bash
cargo run -p apc-cli -- run "dis bonjour en un mot"   # streams the answer
cargo run -p apc-cli -- chat                          # REPL, exit/quit/EOF
cargo run -p apc-cli -- chat --continue                # replay the latest session
printf 'Quelle est la capitale de la France ?\nquit\n' | cargo run -p apc-cli -- chat
```

## CLI surface

```bash
apc init                        # writes a commented apc.toml
apc config                      # effective config + which layer set each key (P1)
apc run "<prompt>"              # v0.2, streams
apc chat [--continue]           # v0.2 REPL, sessions persisted (v0.2b)
apc model list                  # works; says nothing is published (empty catalog)
apc model download <id>         # resumes a .part, verifies SHA-256, then renames
apc model status <id>
apc model verify <id>
apc model remove <id>
```

Models and sessions live under `${XDG_DATA_HOME:-~/.local/share}/aplix/`
(`models/<id>.bin` with `<id>.bin.part` while downloading, `memory.db` for every conversation,
plus any pre-v1.3 `sessions/<id>.jsonl`, imported on first run and then left alone).

## Telemetry (Sentry)

```bash
cargo run -p apc-cli -- config                       # .env is auto-loaded in debug builds
APC_TELEMETRY=1 APC_SENTRY_DSN=http://key@host/1 target/debug/apc config
APC_TELEMETRY=0 target/debug/apc config              # force off, beats any DSN
cargo build --features telemetry                     # compiles the Sentry path in
APC_LOG=debug target/debug/apc run "…"               # local logs, no DSN, no network
```

Since v1.0 `default = []`: a normal build has **no Sentry code**. Work on it with
`cargo build --features telemetry` (and `cargo test --workspace --features telemetry`), plus
`telemetry = true` in the config and a DSN in the environment — both runtime gates still apply.
`.env` (git-ignored) no longer sets `APC_TELEMETRY`; debug builds read it through the 15-line
`load_dotenv` in `main.rs` (release builds never do).

Verify delivery without reading the relay: point the DSN at a one-shot listener and force an error
(broken `apc.toml` is enough).

```bash
python3 -c "import socket;s=socket.socket();s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1);s.bind(('127.0.0.1',8799));s.listen(1);c,_=s.accept();open('/tmp/post.txt','wb').write(c.recv(65535))" &
APC_TELEMETRY=1 APC_SENTRY_DSN=http://pub@127.0.0.1:8799/1 ./target/debug/apc config  # in a dir with a broken apc.toml
```

A `POST /api/1/envelope/` with an `event` payload means wiring works. No DSN ⇒ `ClientOptions::new()`
keeps `dsn = None` ⇒ disabled client, zero network.

The dev DSN in `.env` points at **Rustrak** (`rustrak-api.applix.fr`, project `3` "Aplix code") —
readable back through the `senntry-online` MCP, so delivery can be checked without opening the UI:

```
get_server_version()                       # 0.23.0
list_projects()                            # project id 3, stored_event_count tells you if it arrived
list_issues({ project_id: 3 })             # short_id APLIX-CODE-n, title, level
list_events({ project_id: 3, issue_id })   # event ids
get_event({ project_id: 3, issue_id, event_id })
```

Verified 2026-11-01: a config error landed as `APLIX-CODE-1`, and `warn!(tool failed)` from
`crates/apc-agent/src/lib.rs:73` landed as `APLIX-CODE-2` with `level=warning`, `logger=apc_agent`,
fields `tool` and `error` under "Rust Tracing Fields" — the error string carries the file path.

## Chat view (TUI) — testing it

`apc chat` opens the full-screen view only when stdin and stdout are terminals. A bare `script`
gives the child a 0x0 window, so ratatui draws nothing; set the size on the pty:

```python
# /tmp/drive_tui.py — minimal pty driver
import fcntl, os, pty, select, struct, time
pid, fd = pty.fork()
if pid == 0:
    os.execvp("sh", ["sh", "-c", "./target/debug/apc chat"])
fcntl.ioctl(fd, 0x5414, struct.pack("HHHH", 40, 120, 0, 0))   # TIOCSWINSZ: 120x40
time.sleep(1); os.write(fd, b"dis bonjour\r"); time.sleep(20); os.write(fd, b"\x1b")
while select.select([fd], [], [], 0.1)[0]:
    os.read(fd, 65536)   # strip ANSI / \r to read the screen
```

Verified that way: streaming shows `thinking…` then the text, `run_shell` on `rm -rf …` shows the
`run it? … [y/N]` footer and `n` leaves the probe untouched, and a second turn recalls the first
(history reloaded from the store).

Piped runs stay on the REPL, which is what the tests exercise:
`printf 'bonjour\nquit\n' | ./target/debug/apc chat`.

## Docs lookup (this machine)

Context7 CLI, no global install needed:

```bash
npx ctx7@latest library "Rust" "Cargo workspace dependency inheritance"
npx ctx7@latest docs /websites/rs_genai "tool calling in chat completions"
```

Verified IDs already found: `/websites/rs_genai` (genai, latest 0.5.2), `/jeremychone/rust-genai`
(v0.4.0-alpha.7), `/dtolnay/async-trait`, `/rust-lang/rust`, `/rust-lang/reference`.
