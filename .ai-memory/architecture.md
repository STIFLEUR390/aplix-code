# Architecture (SPEC §4)

v0.3 is shipped end to end: config, providers, sessions, tools and the guard. What remains is the
`open-jev` backend (v0.3b), `apc model` (v0.4), hooks, context compaction and the post-MVP items. `SPEC.md` is the reference; `README.md` §Architecture is the same
layout minus the V1 refinements below.

## Crate layout

```
crates/
  apc-core/      shared types, errors, config         — no internal deps
  apc-traits/    all traits                          — the plugin contract
  apc-agent/     agent loop, orchestration           — no direct I/O
  apc-tools/     built-in tools (fs, shell)
  apc-decision/  Permission Guard: heuristic first, then open-jev
  apc-models/    LLM adapters (direct HTTP + SSE)
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
    async fn chat(&self, messages: &[Message], tools: &[ToolSpec]) -> Result<ChatStream>;
    fn model(&self) -> &str;
}
```
Shipped signature: `chat(&self, messages: &[Message], tools: &[ToolSpec]) -> Result<ChatStream>`.

#[async_trait]
pub trait DecisionProvider: Send + Sync {
    async fn noul(&self, state: &str, question: &str) -> Result<f64>;
    async fn choice(&self, state: &str, question: &str, options: &[&str]) -> Result<(String, f64)>;
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> serde_json::Value;
    async fn execute(&self, args: serde_json::Value) -> Result<String>;
    fn spec(&self) -> ToolSpec;            // provided
}

pub trait Approver: Send + Sync {
    fn confirm(&self, command: &str, risk: f64) -> Result<bool>;
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

## Agent loop (v0.3, shipped)

`apc-agent::Agent { adapter: Arc<dyn ModelAdapter>, tools: Vec<Arc<dyn Tool>> }`:

1. `adapter.chat(conversation, &specs)` and drain the stream, forwarding `ChatEvent`s to the caller's
   callback (the CLI prints deltas as they land).
2. No tool calls ⇒ push `Message::assistant(text)` and stop.
3. Otherwise push `Message::assistant_calls(text, calls)`, run each call, push
   `Message::tool_result(call.id, text)` for every call — including failures and unknown names, which
   the model reads as the answer.
4. At most `MAX_STEPS = 8` tool rounds; beyond that `Error::Tool("stopped after 8 tool steps …")`.

Still to come: hooks (`PreToolUse`, `PostToolUse`, `Stop`) and context compaction.

## Built-in tools (v0.3, shipped)

`crates/apc-tools/src/lib.rs`: `ReadFile`, `WriteFile`, `ListDir`, `RunShell`.

- Paths resolve lexically under the working directory; absolute paths and `..` above the root are
  refused (`resolve()`), so nothing needs to exist yet and no filesystem call decides safety.
- `RunShell` is the only shell in the workspace: `decision.noul(command, GUARD_QUESTION)` then, when
  `risk > threshold`, `approver.confirm(...)`. A refusal returns
  `"the user declined to run this command, so it was not executed: …"` as the tool result.
- `apc-plugins::default_tools(root, decision, approver, threshold)` assembles the four; the CLI never
  names a tool itself.

## CLI surface

| Command | Priority | State |
| :--- | :--- | :--- |
| `apc run "<prompt>"` | P0 | **done** (agent + tools) |
| `apc chat` (plain REPL; `--continue` resumes) | P0 | **done** |
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

## Providers (v0.2, shipped)

`apc-models` speaks HTTP directly — `genai` was dropped (SPEC §8.10). Two dialects:

| Dialect | Providers | Endpoint | Auth |
| :--- | :--- | :--- | :--- |
| `OpenAi` | openai, deepseek, openrouter, groq, ollama | `{base}/chat/completions` | `Authorization: Bearer` (none for ollama) |
| `Anthropic` | anthropic | `{base}/messages` | `x-api-key` + `anthropic-version: 2023-06-01` |

- `defaults()` in `crates/apc-models/src/lib.rs` maps name → (dialect, default base URL, key var);
  `provider.base_url` overrides the URL, which is how the local relay is used.
- Gemini is refused with an explanatory error: it needs a third dialect (v1.1+).
- `ChatStream` = `Pin<Box<dyn Stream<Item = Result<ChatEvent>> + Send>>`; `ChatEvent::{Delta, Done}`.
  `apc-cli::print_stream` writes and flushes each delta.
- `sse.rs` holds the `text/event-stream` splitter (`Sse::push(&mut self, chunk) -> Vec<String>`), unit
  tested on chunk boundaries, CRLF and comments.
- A 200 response whose body is not an event stream is converted into an error carrying the first
  200 chars — a wrong `base_url` otherwise streams nothing and exits 0.

## Session memory (v0.2b, shipped)

`apc-memory::JsonlStore` implements `MemoryStore`: one JSONL file per session under
`ProjectDirs::from("dev","aplix","aplix").data_dir()/sessions/`, a `Message` per line
(`{"role":"user","content":"…"}`), append-only.

- `new_session()` → `{:013}-{:06}` (unix seconds + microseconds): file names sort chronologically, so
  `latest_session()` is `max()` over `read_dir`, no stat calls.
- `load(None)` = latest session; `load(Some(id))` = that file; a missing file/dir loads empty.
- Lines that fail to parse are skipped — a torn append leaves a fragment and must not lose the rest.
- `apc chat` appends the user turn *before* calling the provider (a crash keeps the prompt) and the
  full assistant reply after the stream ends. `apc run` does not persist anything.
- `--continue` prints `continuing session <id> (N messages)` to stderr, then replays into context.
- Long-term memory (schema, forgetting, distillation) is v1.3; the same trait will take it.

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
