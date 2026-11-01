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

> **⚠️ Early-stage project.** `aplix code` is currently under active development. The architecture, traits, and CLI interface are still being stabilized. Expect breaking changes until `v1.0.0`. Star ⭐ the repo to follow the journey.

---

## 🎯 What is aplix code?

`aplix code` (or **`apc`** for short) is a **coding agent that runs in your terminal**. It talks to your favorite LLM (OpenAI, Anthropic, Gemini, Ollama, DeepSeek, OpenRouter, …), reads and writes files, runs shell commands, and helps you get real work done.

But unlike the existing tools, `apc` was built around three core principles:

1. **Local-first by default.** Micro-decisions (is this command dangerous? what does the user want? is this context relevant?) are made **locally**, in under 20 ms, with a small neural decision model — no API call, no latency, no cloud, no cost.
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

### ✅ Working today (`v1.0`)

- **`apc run "<prompt>"`** — One-shot agent mode.
- **`apc chat`** — Interactive REPL in the terminal.
- **`apc init`** — Generate a commented `apc.toml` config file.
- **`apc config`** — Print the effective configuration and which layer supplied each key.
- **Built-in tools**: `read_file`, `write_file`, `list_dir`, `run_shell`.
- **Local Permission Guard** — Every shell command is scored before it runs (`rm -rf`, `DROP TABLE`, `mkfs`…), and anything above the threshold needs an explicit confirmation. The built-in heuristics need no model at all; building with `--features open-jev` lets a local-jev sidecar answer first and falls back to them on any failure.
- **Multi-provider support** over the two dialects in use (`/chat/completions` compatible OpenAI, `/messages` Anthropic): OpenAI, Anthropic, Ollama, DeepSeek, OpenRouter, Groq, and any compatible local gateway.
- **Session persistence** — Resume a conversation with `apc chat --continue`.
- **Opt-in error reporting** — Sentry-compatible, compiled out by default (`--features telemetry` to build it in), off without a DSN and a config flag.
- **`apc model <cmd>`** — Manage local decision models (`list`, `download`, `status`, `verify`, `remove`), with resumable, SHA-256 verified downloads. The catalog is empty for now: no upstream artifact exists (`SPEC.md` §8.12).
- **CI and releases** — `fmt`, `clippy -D warnings` and tests in three feature shapes on every push, a 15 MB size gate, and tagged builds for Linux, macOS and Windows.

### 📦 Still planned

- **crates.io publication** — `apc` installs from source or a release binary today (`SPEC.md` §7).

### 🔮 Post-MVP (V1.x – V2)

- Rich TUI with [`ratatui`](https://crates.io/crates/ratatui).
- Local memory (SQLite + `sqlite-vec` for RAG over past sessions).
- **SearXNG** web search (self-hosted, private).
- **Obscura**-based web extraction.
- Dynamic plugin loading (native `.so` or WASM).
- `apc serve` — local HTTP server, for editor integrations.
- **Tauri desktop app** (a thin client over `apc serve`).
- Advanced sandboxing (Landlock / Seatbelt).

---

## 🚀 Quick start

> **Note:** `apc` is not yet on crates.io. Use a release binary, or install from source.

```bash
# Option A — prebuilt binary: https://github.com/STIFLEUR390/aplix-code/releases
#   apc-Linux-X64.tar.gz, apc-macOS-ARM64.tar.gz, apc-Windows-X64.zip (+ .sha256)

# Option B — from source (Rust 1.80+)
git clone https://github.com/STIFLEUR390/aplix-code.git
cd aplix-code
cargo install --path crates/apc-cli --locked   # add --offline once the crates are cached

# Configure any OpenAI-compatible provider
export OPENAI_API_KEY="sk-..."

apc init          # writes a commented apc.toml
apc config        # every value, and which layer supplied it

# Start coding
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
│   ├── apc-memory/     # Session history (JSONL); long-term memory lands in v1.3.
│   ├── apc-plugins/    # Static plugin registry (dynamic loading lands in v1.2).
│   └── apc-cli/        # The `apc` binary.
└── skills/             # Markdown-based skill definitions.
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
  reporting: off by default, no DSN shipped, no user content — see `SPEC.md` §3.4 and §8.9.
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

While building offline is what this machine needs, `cargo build --offline` is the safe form; the
`open-jev` decision backend is a separate opt-in feature (`--features open-jev`), see `SPEC.md` §8.11.

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
| **v1.0** | MVP: stable CLI, docs, CI and release artifacts for Linux/macOS/Windows |
| **v1.1** | `ratatui`-based TUI, intent classification, `skills/` |
| **v1.2** | SearXNG + Obscura plugins |
| **v1.3** | Long-term memory (SQLite + FTS5 / `sqlite-vec`) |
| **v2.0** | `apc serve` + Tauri desktop app |
| **v2.1** | Advanced sandboxing (Landlock/Seatbelt) or WASM plugins |

---

## 🤝 Contributing

`apc` is at the very start of its life, which means **there's never been a better time to shape it**.

- **Discussions & ideas**: open an issue with the `idea` label.
- **Bug reports**: please use the template in `.github/ISSUE_TEMPLATE/`.
- **Pull requests**: read `CONTRIBUTING.md` first (coming soon).
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
- **[harness-rs](https://github.com/harness-rs/harness)** — for the most elegant "everything is a plugin" agent architecture in Rust.
- **[OpenCode](https://github.com/sst/opencode)** — for the server-first, client-agnostic architecture we're adopting for `apc serve`.
- **[local-jev](https://github.com/ronitgupta138/local-jev)** — for proving that fast, local decision models are not only possible, but practical.
- **[rig](https://github.com/0xPlaydust/rig)** and the Rust AI ecosystem — for showing what agent loops and provider abstractions can look like.

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
