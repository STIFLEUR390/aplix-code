# Architecture (SPEC §4)

v0.1 implements the bottom of the stack: `apc-core` (config) and `apc-cli` (binary). The rest is the
contract to implement against. `SPEC.md` is the reference; `README.md` §Architecture is the same
layout minus the V1 refinements below.

## Crate layout

```
crates/
  apc-core/      shared types, errors, config         — no internal deps
  apc-traits/    all traits                          — the plugin contract
  apc-agent/     agent loop, orchestration           — no direct I/O
  apc-tools/     built-in tools (fs, shell)
  apc-decision/  Permission Guard: heuristic first, then open-jev
  apc-models/    LLM adapters (genai)
  apc-memory/    JSONL session store (+ future backends)
  apc-plugins/   static plugin registry
  apc-cli/       the `apc` binary — composition root
skills/          Markdown skill definitions (v1.1+, does not exist yet)
```

Dependency direction: `apc-core` at the bottom, `apc-traits` above it, plugin crates depend on both,
`apc-agent` knows only the traits, `apc-cli` wires concrete implementations together. `apc-agent`
and `apc-cli` must stay free of provider- or backend-specific dependencies.

`apc-plugins` in V1 is a **static registry** (name → `Box<dyn Tool>` / `Box<dyn DecisionProvider>`).
It exists so `apc-cli` doesn't hardcode tool wiring, and so the v1.2 dynamic loader can land without
changing callers.

## The five traits

```rust
#[async_trait]
pub trait ModelAdapter: Send + Sync {
    async fn chat(&self, messages: &[Message], tools: &[Tool]) -> Result<ChatStream>;
}

#[async_trait]
pub trait DecisionProvider: Send + Sync {
    async fn noul(&self, state: &str, question: &str) -> Result<f64>;
    async fn choice(&self, state: &str, question: &str, options: &[&str]) -> Result<(String, f64)>;
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn schema(&self) -> serde_json::Value;
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult>;
}

pub trait MemoryStore: Send + Sync {
    async fn append(&self, session: &SessionId, message: &Message) -> Result<()>;
    async fn load(&self, session: Option<&SessionId>) -> Result<Vec<Message>>;
}

pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn init(&self, ctx: &mut PluginContext) -> Result<()>;
}
```

`noul` returns a confidence for a yes/no question about `state`; `choice` picks one option plus its
confidence. Keep the names exactly as written (`noul` comes from the local-jev lineage).
`chat` returns a **stream**, not a full response (SPEC §8.3).

## Agent loop

ReAct: `Observation → Reflection → Action`, with error handling, timeouts and a per-turn iteration
cap. Hooks reserved for lifecycle events (`PreToolUse`, `PostToolUse`, `Stop`) — not user-exposed in
V1, but the extension point must exist.

Context compaction: summarize older messages when approaching the model window.

## Built-in tools (P0)

`read_file`, `write_file` (confirmation if the file exists), `list_dir`, `run_shell` (Permission
Guard only).

## CLI surface

| Command | Priority | State |
| :--- | :--- | :--- |
| `apc run "<prompt>"` | P0 | v0.2 |
| `apc chat` (plain REPL; `--continue` resumes) | P0 | v0.2 |
| `apc init` | P0 | **done** |
| `apc model list\|download\|status\|verify\|remove` | P0 | v0.4 |
| `apc --version` (also prints the loaded decision model) | P0 | **done** (`decision model: none (heuristic fallback)`) |
| `apc config` (effective config + which layer set it) | P1 | **done** |

## Config (v0.1)

`apc-core::config`:

- `Config { provider { name, model }, decision { threshold }, telemetry }`, `Serialize`/`Deserialize`
  with `#[serde(default, deny_unknown_fields)]`.
- `Layer { source, table }` + `Source { Defaults, User, Project, Local, Env, Flag }` (declaration
  order *is* the precedence order). Defaults are themselves a `Layer`, so provenance has no special
  case.
- `resolve(&[Layer]) -> Loaded { config, sources }`: deep-merges tables, then walks each layer's
  leaves to record `sources["decision.threshold"] = Source::Project`.
- `TEMPLATE` = the commented file written by `apc init`, and the documented key list.
- `scalar`/`boolean` parse `APC_*` strings (env vars are strings, TOML is typed).

`apc-cli::main` owns the I/O: `ProjectDirs::from("dev", "aplix", "aplix")` for the user file, flags
via clap `Args`. No precedence logic there.

## Providers

Through `genai`: OpenAI, Anthropic, Gemini, Ollama, DeepSeek, OpenRouter, Groq. Provider + model in
`apc.toml`; keys from env (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …).

## Session memory

JSONL under `${XDG_DATA_HOME:-~/.local/share}/aplix/sessions/`, one file per session. `apc chat
--continue` reloads the latest. No long-term memory in V1 (v1.3).

## Telemetry (Sentry)

Implemented in `crates/apc-cli/src/telemetry.rs` (the whole crate is the composition root, per SPEC
§3.4). `init(enabled) -> Guard` returns the `ClientInitGuard`, held for the life of `main` so queued
events flush on drop; `capture(&anyhow::Error)` reports a command failure before it is printed.

Gates, in order: `telemetry` feature compiled (currently a **default** feature during development) →
`telemetry = true` in the resolved config → a DSN in `APC_SENTRY_DSN`/`SENTRY_DSN`. `main` decides
*before* running the command so that a config that fails to load is itself reportable; `apc init` has
no config yet, so it falls back to the env variable alone.

Not implemented yet: the `tracing` fmt layer (arrives with the first real logs in v0.2) and
`warn`/`error` breadcrumbs. Today Sentry receives explicit errors and, in debug builds, panics
(`panic = "abort"` disables the panic hook in release).

Rules: no DSN shipped, `APC_TELEMETRY=0` beats any DSN, no usage metrics
(`auto_session_tracking: false`), never file/prompt/completion content. Libraries do not initialise a
client; only `apc-cli` does.

## Post-MVP (do not build during an MVP task)

`ratatui` TUI (v1.1), intent classification (v1.1), `skills/` (v1.1), SearXNG + Obscura plugins
(v1.2), SQLite/`sqlite-vec` long-term memory (v1.3), `apc serve` + Tauri (v2.0), Landlock/Seatbelt
or WASM sandboxing (v2.1), dynamic plugin loading (v1.2).
