# Commands

v0.3b is implemented; commands marked *target* do not exist yet.

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

*Target (v0.4+):* `apc model <cmd>`.

Requires Rust 1.80+. Only system dependency expected: OpenSSL, or `rustls` if the feature is enabled.

## Verify (in this order before claiming done)

```bash
cargo fmt --all --check
cargo clippy --offline --all-targets -- -D warnings
cargo test --offline --workspace
cargo test --offline -p apc-decision --features open-jev   # the open-jev build is a separate path
cargo build --offline --no-default-features               # telemetry off
```


```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
ls -lh target/release/apc             # must stay under 15 MB (893 KB at v0.1)
```

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
apc model list                  # target (v0.4)
apc model download open-jev     # ~350 MB, resumable + SHA-256 verified
apc model status
apc model verify
apc model remove open-jev
```

Model cache and sessions live under `${XDG_DATA_HOME:-~/.local/share}/aplix/`
(sessions in `sessions/<id>.jsonl`, one file per conversation).

## Telemetry (Sentry)

```bash
cargo run -p apc-cli -- config                       # .env is auto-loaded in debug builds
APC_TELEMETRY=1 APC_SENTRY_DSN=http://key@host/1 target/debug/apc config
APC_TELEMETRY=0 target/debug/apc config              # force off, beats any DSN
cargo build --no-default-features                    # compiles the `telemetry` feature out
```

Current state (dev phase): `telemetry` is a **default** feature of `apc-cli`, `.env` (git-ignored)
sets `APC_TELEMETRY=1` + `http://…@localhost:8789/5`, and debug builds read `.env` from the current
directory through the 15-line `load_dotenv` in `main.rs` (release builds never call it). Runtime
still needs **both** gates: `telemetry = true` in the resolved config and a DSN in the environment.

Verify delivery without reading the relay: point the DSN at a one-shot listener and force an error
(broken `apc.toml` is enough).

```bash
python3 -c "import socket;s=socket.socket();s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1);s.bind(('127.0.0.1',8799));s.listen(1);c,_=s.accept();open('/tmp/post.txt','wb').write(c.recv(65535))" &
APC_TELEMETRY=1 APC_SENTRY_DSN=http://pub@127.0.0.1:8799/1 ./target/debug/apc config  # in a dir with a broken apc.toml
```

A `POST /api/1/envelope/` with an `event` payload means wiring works. No DSN ⇒ `ClientOptions::new()`
keeps `dsn = None` ⇒ disabled client, zero network.

## Docs lookup (this machine)

Context7 CLI, no global install needed:

```bash
npx ctx7@latest library "Rust" "Cargo workspace dependency inheritance"
npx ctx7@latest docs /websites/rs_genai "tool calling in chat completions"
```

Verified IDs already found: `/websites/rs_genai` (genai, latest 0.5.2), `/jeremychone/rust-genai`
(v0.4.0-alpha.7), `/dtolnay/async-trait`, `/rust-lang/rust`, `/rust-lang/reference`.
