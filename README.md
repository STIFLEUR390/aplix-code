<div align="center">

# ⚡ aplix code (`apc`)

**A local-first, plugin-driven coding agent for your terminal.**

[![Status](https://img.shields.io/badge/status-work%20in%20progress-orange?style=flat-square)](https://github.com/STIFLEUR390/aplix-code)
[![CI](https://github.com/STIFLEUR390/aplix-code/actions/workflows/ci.yml/badge.svg)](https://github.com/STIFLEUR390/aplix-code/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen?style=flat-square)](https://github.com/STIFLEUR390/aplix-code/pulls)

</div>

---

> **⚠️ Early-stage project.** `v1.0.0` ships the MVP feature set as a prebuilt binary or from source. The crate is not published to crates.io yet, and the APIs may still move before `v2.0`. Star ⭐ the repo to follow the journey.

---

## 🎯 What is aplix code?

`aplix code` (or **`apc`** for short) is a **coding agent that runs in your terminal**. It talks to your favorite LLM (OpenAI, Anthropic, Ollama, DeepSeek, OpenRouter, Groq, or any OpenAI-compatible gateway), reads and writes files, runs shell commands, and helps you get real work done.

But unlike the existing tools, `apc` was built around three core principles:

1. **Local-first by default.** Micro-decisions (is this command dangerous? what does the user want? is this context relevant?) are made **locally**, in under 20 ms, with no API call, no latency, no cloud, no cost. Today that is a built-in heuristic; a small neural decision model is the target.
2. **Everything is a plugin.** Every layer of the agent — the model adapter, the tools, the memory backend, the decision engine — is defined by a trait and can be swapped. You are never locked in.
3. **Lite by design.** The CLI binary stays **under 15 MB**. Models are downloaded on demand, with resumable, checksum-verified downloads. No 500 MB bundle on `cargo install`.

---

## 💡 Why another coding CLI?

The space is crowded, and that's a good thing. `apc` doesn't try to be a better `claude code` or a lighter `codex-cli`. It tries to solve a specific pain point: **the cost and latency of micro-decisions inside an agent loop**.

Every modern coding agent makes hundreds of tiny decisions per session:

- *"Is this shell command safe to run?"*
- *"What does the user actually want here — generation, explanation, or review?"*
- *"Is this 40 KB of context relevant to the question?"*

Sending each of these to a cloud LLM is slow, expensive, and leaky. `apc` answers them **locally, in under 20 ms, for free**, and only calls the big LLM when it's actually needed. Today that is a built-in heuristic guard, which needs no model and never blocks; the target is `open-jev`, a quantized DeBERTa-v3 reached over a local-jev sidecar, with the heuristic kept as the permanent fallback.

| | Traditional CLI agent | **`apc`** |
| :--- | :---: | :---: |
| **Cost of a safety check** | ~$0.0005 + 150 ms | **$0 + 5 ms** |
| **Privacy of internal context** | Sent to cloud | **Stays on device** |
| **Works offline** | ❌ | ✅ (for decisions) |
| **Binary size** | 100–200 MB | **< 15 MB** |
| **Plugin model** | Some | **Everything** |

---

## ✨ Features

### ✅ Working today (`v2.4`)

- **`apc run "<prompt>"`** — One-shot agent mode: streams the answer, calls tools, guards shell.
- **`apc chat`** — Full-screen chat view (`ratatui`): scrollable transcript, a status bar, slash
  commands (`/mode plan|build`, `/model`, `/skills`, `/skill`, `/help`), live streaming, the
  Permission Guard prompt in the status bar; falls back to a plain REPL when stdin/stdout is piped.
- **`apc init`** — Generate a commented `apc.toml` config file.
- **`apc config`** — Print the effective configuration and which layer supplied each key.
- **`apc setting`** — Write a setting without editing TOML by hand: `apc setting telemetry on`,
  `apc setting decision.threshold 0.9`, `apc setting tools.searxng_url http://127.0.0.1:8888`.
  Goes to `~/.config/aplix/apc.toml` (or `./apc.toml` with `--project`), refuses an unknown key, and
  rewrites the file without its comments. `apc setting` alone lists every key with the layer it came
  from.
- **`apc serve`** — The same agent behind a loopback HTTP API (`127.0.0.1:7717`, SSE), gated by a
  token written to `serve.token`: `POST /v1/run`, `GET /v1/sessions`, `GET /v1/config`,
  `GET /v1/providers` and `POST /v1/confirm`, which is how a desktop client answers the Permission
  Guard instead of a terminal prompt.
- **`apc-desktop`** — A Tauri window over that API: streamed transcript, provider and model picker,
  a stop button, the guard prompt inline. It is a separate crate excluded from the root workspace, so
  the `apc` binary never links GTK or WebKit.
- **Built-in tools**: `read_file`, `write_file`, `list_dir`, `run_shell`, `fetch_page` (main-content
  extraction as Markdown via `rs-trafilatura`, boilerplate dropped). Every tool result is capped
  (`tools.max_output_bytes`, 32 KB; 64 KB for a fetched page) and ends with
  `[truncated at N of M bytes]` when it was cut.
- **Honest context handling** — every turn re-sends the session as it stands: no compaction, no
  hidden summarisation, nothing silently dropped. `MAX_STEPS` (8 tool rounds) and the tool-result
  caps bound one turn; a very long session is not compacted yet (`SPEC.md` §8.16).
- **Private web search** — `web_search` goes through your own SearXNG instance
  (`tools.searxng_url` / `APC_SEARXNG_URL`); without one the tool is not registered at all, so the
  model is never offered a tool that cannot answer.
- **Intent classification and skills** — A local heuristic tags each prompt (`generate_code`,
  `explain_code`, `review_code`, `other`), no model needed, and the matching `skills/<name>.md` of the
  current project heads the conversation as the system prompt. No skill file, no system prompt.
- **Local Permission Guard** — Every shell command is scored before it runs (`rm -rf`, `DROP TABLE`, `mkfs`…), and anything above the threshold needs an explicit confirmation. The built-in heuristics need no model at all; building with `--features open-jev` lets a local-jev sidecar answer first and falls back to them on any failure.
- **Multi-provider support** over the two dialects in use (`/chat/completions` compatible OpenAI, `/messages` Anthropic): OpenAI, Anthropic, Ollama, DeepSeek, OpenRouter, Groq, and any compatible local gateway.
- **Session persistence** — Resume a conversation with `apc chat --continue`; sessions live in SQLite
  at `${XDG_DATA_HOME:-~/.local/share}/aplix/memory.db`, and each turn is prefixed with what earlier
  sessions said about the prompt (FTS5).
- **Local logs** — `APC_LOG=debug|info|warn|error` on stderr, no network, no DSN needed.
- **Opt-in error reporting** — Sentry-compatible, compiled out by default (`--features telemetry` to build it in), off without a DSN and a config flag.
- **`apc model <cmd>`** — Manage local decision models (`list`, `download [id]`, `status`, `verify`, `remove`), with resumable, SHA-256 verified downloads; `apc model download` with no id resumes every one of them. The catalog is empty for now: no upstream artifact exists (`SPEC.md` §8.12).
- **CI and releases** — `fmt`, `clippy -D warnings` and tests in three feature shapes on every push, a 15 MB size gate, and tagged builds for Linux, macOS and Windows.

### 📦 Still planned

- **crates.io publication** — `apc` installs from source, a release binary, or `install.sh` today
  (`SPEC.md` §7).

### 🔮 Post-MVP (V1.x – V2)

- Vector recall over past sessions (`sqlite-vec`), once embeddings are computed locally.
- JavaScript-rendered page extraction (CDP or `obscura mcp --http`, `SPEC.md` §8.13).
- Dynamic plugin loading (native `.so` or WASM).
- Packaged desktop builds (`tauri bundle`): `crates/apc-desktop` is a working window today, but this
  repo does not produce installers.
- Advanced sandboxing (Landlock / Seatbelt).

---

## 🚀 Quick start

### Install

**Linux and macOS**

```bash
curl -fsSL https://raw.githubusercontent.com/STIFLEUR390/aplix-code/main/install.sh | sh
```

**Windows** (PowerShell 5.1+, which is every Windows 10 and later)

```powershell
irm https://raw.githubusercontent.com/STIFLEUR390/aplix-code/main/install.ps1 | iex
```

That is the whole install on both: it picks the archive for your OS and architecture, verifies its
SHA-256 against the published `.sha256`, and puts the binary where you can run it — `~/.local/bin` on
Unix, `%LOCALAPPDATA%\apc` on Windows. No sudo, no administrator prompt, no system dependency: TLS
is `rustls`, there is no OpenSSL to find.

| Variable | Effect |
| :--- | :--- |
| `APC_INSTALL_DIR` | Where the binary goes (default `~/.local/bin`, or `%LOCALAPPDATA%\apc`). |
| `APC_VERSION` | Pin a release tag (`v1.2.0`) instead of the latest one. |

If the script says the directory is not in your `PATH`, it prints the exact line to add —
`export PATH="$HOME/.local/bin:$PATH"` in your shell profile, or a `SetEnvironmentVariable` call for
the Windows user `PATH` (then open a new terminal).

**Update** is the same command. Re-run it and it replaces the binary, printing the version it is
replacing first. `APC_INSTALL_DIR` has to match the one you installed with.

### Uninstall

**Linux and macOS**

```bash
curl -fsSL https://raw.githubusercontent.com/STIFLEUR390/aplix-code/main/uninstall.sh | sh
# ...or, from a checkout: sh uninstall.sh [--purge]
```

**Windows**

```powershell
irm https://raw.githubusercontent.com/STIFLEUR390/aplix-code/main/uninstall.ps1 | iex
# with --purge (piped input cannot take the switch):
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/STIFLEUR390/aplix-code/main/uninstall.ps1))) -Purge
```

The binary goes; your config, sessions and models stay. `--purge` / `-Purge` also deletes
`~/.config/aplix` (the `apc.toml` you wrote), `~/.local/share/aplix` (`memory.db`, the serve token,
downloaded models), or on Windows their `%APPDATA%\aplix\aplix` equivalent. Nothing else is
touched — no `PATH` edit, no shell profile edit.

Prefer to do it by hand? Grab `apc-Linux-X64.tar.gz`, `apc-macOS-ARM64.tar.gz` or
`apc-Windows-X64.zip` from [the releases page](https://github.com/STIFLEUR390/aplix-code/releases) —
each ships with its `.sha256`.

### From source

```bash
git clone https://github.com/STIFLEUR390/aplix-code.git
cd aplix-code
cargo install --path crates/apc-cli --locked   # add --offline once the crates are cached
```

### First run

```bash
export OPENAI_API_KEY="sk-..."   # or point apc.toml at Ollama / any compatible gateway

apc init          # writes a commented apc.toml
apc config        # every value, and which layer supplied it
apc setting telemetry on   # write one setting into ~/.config/aplix/apc.toml

apc run "Add a progress bar to the TUI in src/ui.rs"
```

Once it is published, the install is `cargo install aplix-code`. Published binaries are built with
`--no-default-features`, so they carry no Sentry code at all. `apc model` ships today but its
catalog is empty — there is no decision-model artifact upstream yet (`SPEC.md` §8.12). A local-jev
sidecar can still refine the guard: build with `--features open-jev` and set `decision.endpoint`
(see `SPEC.md` §8.11).

---

## 🧱 Architecture

`apc` is a **Cargo workspace** with clear separation of concerns. Each crate has one job, and the philosophy *"everything is a plugin"* is enforced at the trait level, in `apc-traits`.

```
aplix-code/
├── crates/
│   ├── apc-core/       # Shared types, errors, config. No internal deps.
│   ├── apc-traits/     # All the traits (Model, Tool, Memory, Decision, Plugin).
│   ├── apc-agent/      # The agent loop. Orchestration only.
│   ├── apc-tools/      # Built-in tools (fs, shell).
│   ├── apc-decision/   # Heuristic guard + `open-jev` client (feature-gated).
│   ├── apc-models/     # LLM provider adapters (direct HTTP, SSE streaming).
│   ├── apc-memory/     # Session history + FTS5 recall (SQLite, imports old JSONL sessions).
│   ├── apc-plugins/    # Static plugin registry (dynamic loading lands in v1.2).
│   ├── apc-cli/        # The `apc` binary: run, chat, serve, model, setting, config, init.
│   └── apc-desktop/    # Optional Tauri window over `apc serve`; not in the root workspace.
└── .ai-memory/         # Notes for coding agents (start at `index.md`).
```

**Key traits** (simplified — `SPEC.md` §4 is authoritative, including the streaming return type):

```rust
#[async_trait]
pub trait ModelAdapter: Send + Sync {
    async fn chat(&self, messages: &[Message], tools: &[ToolSpec]) -> Result<ChatStream>;
}

#[async_trait]
pub trait DecisionProvider: Send + Sync {
    async fn noul(&self, state: &str, question: &str) -> Result<f64>;
    async fn choice(&self, state: &str, question: &str, options: &[&str]) -> Result<(String, f64)>;
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn schema(&self) -> serde_json::Value;
    async fn execute(&self, args: serde_json::Value) -> Result<String>;
}

#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn append(&self, session: &SessionId, message: &Message) -> Result<()>;
    async fn load(&self, session: Option<&SessionId>) -> Result<Vec<Message>>;
}

pub trait Approver: Send + Sync {
    fn confirm(&self, command: &str, risk: f64) -> Result<bool>;  // the Permission Guard's terminal prompt
}

pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn init(&self, ctx: &mut PluginContext) -> Result<()>;
}
```

---

## 🔒 Security posture

- **No `unsafe`** at all: `#![forbid(unsafe_code)]` in every crate.
- **Local Permission Guard** on every shell command, with configurable threshold.
- **No telemetry, no analytics, no phone-home**. The only exception is opt-in Sentry error
  reporting: compiled out unless the build enables `--features telemetry`, and still inert without
  both `telemetry = true` in the config and a DSN in the environment. Only `warn!`/`error!` events
  and captured command errors leave the process — no file contents, no prompts, no completions
  (tracing fields such as a failing path do travel; keep them out of `warn!`) — `SPEC.md` §3.4, §8.9.
- **Checksum-verified downloads** for every model.
- **Explicit, layered configuration** (defaults < user < project < local < env < flags).

---

## 🛠️ Building from source

```bash
git clone https://github.com/STIFLEUR390/aplix-code.git
cd aplix-code
cargo build --release
./target/release/apc --version
```

Requirements: Rust 1.80+, a modern OS. No system dependencies: TLS is `rustls`, there is no OpenSSL.

Two opt-in features, both off by default:

| Feature | Effect |
| :--- | :--- |
| `open-jev` | Asks a local-jev sidecar (`decision.endpoint`) before the heuristics; falls back on any failure (`SPEC.md` §8.11). |
| `telemetry` | Compiles in Sentry reporting; still requires the config flag and a DSN (`SPEC.md` §3.4). |

```bash
cargo build --release --features open-jev,telemetry
cargo test --workspace                        # fmt + clippy -D warnings are part of CI (`.github/workflows/ci.yml`)
```

---

## 🗺️ Roadmap

| Version | Milestone |
| :--- | :--- |
| **v0.1** | Skeleton, workspace, `apc init` + config resolution |
| **v0.2** | `apc run` + `apc chat` with streaming over direct HTTP adapters |
| **v0.2b** | Session memory in JSONL, `apc chat --continue` |
| **v0.3** | Local Permission Guard: heuristics on every shell command |
| **v0.3b** | `open-jev` backend behind a feature flag, fail-open to the heuristics |
| **v0.4** | `apc model` subcommand + resumable, verified downloads |
| **v0.5** | Opt-in telemetry: local `tracing` logs + Sentry, off by default |
| **v1.0** | MVP: CLI, docs, CI matrix and cross-platform release builds |
| **v1.1** | `ratatui` TUI, intent classification, project `skills/` |
| **v1.2** | SearXNG web search + `fetch_page` (main-content extraction) |
| **v1.2b** | Decided against `rig`: the adapters and the agent loop stay ours (`SPEC.md` §8.2) |
| **v1.3** | Long-term memory (SQLite + FTS5, old JSONL sessions imported) |
| **v2.0** | `apc serve` (loopback + token, SSE) + `apc-desktop`, a Tauri window over it |
| **v2.1** | Advanced sandboxing (Landlock/Seatbelt) or WASM plugins |
| **v2.2** | Chat view modes (`plan`/`build`), model picker, slash commands |
| **v2.3** | `apc setting`, and `apc model download` resuming every published model |
| **v2.4** | Every tool result capped (`tools.max_output_bytes`), `SPEC.md` §8.16 |

---

## 🤝 Contributing

`apc` is at the very start of its life, which means **there's never been a better time to shape it**.

- **Discussions & ideas**: open an issue with the `idea` label.
- **Bug reports**: open an issue with what you ran, what you expected, and the `APC_LOG=debug` output.
- **Pull requests**: keep `cargo fmt`, `cargo clippy -D warnings` and `cargo test --workspace` green
  in all three feature shapes CI builds; there is no `CONTRIBUTING.md` yet.
- **Design principles**: any PR must respect the traits in `apc-traits` and the "everything is a plugin" philosophy.

If you want to help but don't know where to start, look for issues tagged [`good first issue`](https://github.com/STIFLEUR390/aplix-code/labels/good%20first%20issue).

---

## 💬 Community

- **GitHub Discussions**: for longer-form proposals.

---

## 🙏 Inspirations & acknowledgements

`apc` stands on the shoulders of giants. We are deeply grateful to the projects whose ideas, code, and community have shaped our thinking:

- **[Codex CLI](https://github.com/openai/codex)** — for showing how far sandboxing and modular workspace design can go.
- **[Claude Code](https://docs.anthropic.com/en/docs/claude-code)** — for the reference UX of a coding agent.
- **[harness-rs](https://github.com/liliang-cn/harness-rs)** — for the most elegant "everything is a plugin" agent architecture in Rust.
- **[OpenCode](https://github.com/anomalyco/opencode)** — for the server-first, client-agnostic architecture we're adopting for `apc serve`.
- **[local-jev](https://github.com/ronitgupta138/Local-Jev)** — for the minimum/choice decision API our `DecisionProvider` trait is modelled on.
- **[rig](https://github.com/0xPlaygrounds/rig)** ([docs](https://rig.rs/docs)) and the Rust AI ecosystem — for showing what agent loops and provider abstractions can look like.

If we forgot someone, please open an issue — it's not intentional.

---

## 📄 License

MIT — see [LICENSE](LICENSE).

`aplix code` is an independent, community-driven project. It is not affiliated with OpenAI, Anthropic, or any of the projects listed above.

---

<div align="center">

**Built with ❤️ and a lot of `cargo check` in Rust.**

*Star the repo if you believe local-first agents are the future.*

</div>
