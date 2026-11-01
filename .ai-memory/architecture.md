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
  apc-tools/     built-in tools (fs, shell, web)
  apc-decision/  Permission Guard: heuristic first, then open-jev
  apc-models/    LLM adapters (direct HTTP + SSE)
  apc-memory/    JSONL session store (+ future backends)
  apc-plugins/   static plugin registry
  apc-cli/       the `apc` binary — composition root
skills/          Markdown skill definitions, read as the system prompt (v1.1)
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

## Decision backends (v0.3b, shipped)

- `apc_traits::DecisionProvider`: `noul(state, question) -> P(yes)` and `choice(state, question,
  options) -> (pick, confidence)`.
- `HeuristicProvider` (always built): `score()` over `PATTERNS`, the permanent fallback.
- `JevProvider` (feature `open-jev`, `crates/apc-decision/src/jev.rs`): HTTP client for a local-jev
  sidecar — `POST {endpoint}/v1/systemone/choice` with `{state, question, options}`; `noul` is a
  `["no","yes"]` choice read on `probabilities[option == "yes"].probability`.
- `Cascade { primary, fallback }`: primary first, fallback on any error (fail-open).
- CLI wiring: `decision_provider(config)` in `crates/apc-cli/src/main.rs` — with the feature and a
  `decision.endpoint`, the cascade wraps `JevProvider`; otherwise the heuristic alone.

## Built-in tools (v0.3, shipped)

`crates/apc-tools/src/lib.rs`: `ReadFile`, `WriteFile`, `ListDir`, `RunShell`;
`crates/apc-tools/src/web.rs`: `WebSearch` (SearXNG) and `FetchPage` (plain HTTP, `SPEC.md` §8.13).
`apc_plugins::default_tools` omits `web_search` when `tools.searxng_url` is unset.

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
| `apc chat` (full-screen view, or REPL when piped; `--continue` resumes) | P0 | **done** (v1.1 view) |
| `apc init` | P0 | **done** |
| `apc model list\|download\|status\|verify\|remove` | P0 | **done** (v0.4; empty catalog) |
| `apc --version` (also prints the loaded decision model) | P0 | **done** (`decision model: none (heuristic fallback)`) |
| `apc config` (effective config + which layer set it) | P1 | **done** |

## Chat view (v1.1, `crates/apc-cli/src/tui.rs`)

- `tui::available()` = stdin **and** stdout are terminals; anything piped (all CLI tests) takes
  `main::repl`, the untouched stdin/stdout loop.
- State: `App { session, transcript: String, input, mode: Idle | Busy(rx), pending: Option<Ask>,
  tick, scroll }`. One loop per tick: pick up a pending guard question, draw, drain key events, drain
  the agent channel, `sleep(50ms)`.
- `Event` from the agent task is only `Delta(String)` and `Done(Option<String>)`: history never
  travels back through the view. `run_turn` reloads the session with `JsonlStore::load(Some(&id))`,
  appends the prompt, runs the agent, appends what it produced.
- `TuiApprover(Sender<Ask>)` implements `Approver`; `confirm` blocks on a `std::sync::mpsc` reply
  channel inside the agent task while the render loop shows the question in the footer and answers
  on `y`/`n`. Poll the ask channel with `try_recv()`, never `recv()` from the async loop.
- Scroll offset: `wrapped_height(text, width)` (per line `chars/width`, min 1) because
  `Paragraph::line_count` is private in ratatui 0.29.

## Model store (v0.4, shipped)

`crates/apc-decision/src/store.rs`:

- `ModelEntry { id, url, sha256, bytes }` and `CATALOG: &[ModelEntry]`, **empty**: `local-jev` ships
  no weights (SPEC §8.11), so there is nothing to download; inventing a URL would make the command
  lie (SPEC §8.12). `find(id)` returns `Error::Model("unknown model …")` otherwise.
- `Store` rooted at `ProjectDirs::data_dir()/models`; `status()` = `Missing | Partial(bytes) |
  Installed`; `download(entry, progress)` writes `<id>.bin.part` with a `Range: bytes=N-` resume,
  truncates and restarts if the server answers `200`, refuses to rename unless the byte count and
  the SHA-256 match, and deletes a corrupt `.part`; `verify()` re-hashes an installed file;
  `remove()` deletes file + partial.
- Cost: `reqwest` + `sha2` became unconditional dependencies of `apc-decision`; only `serde_json`
  stays behind `open-jev`.

## Config (v0.1)

`apc-core::config`:

- `Config { provider { name, model, base_url }, decision { threshold, endpoint }, telemetry }`,
  `Serialize`/`Deserialize` with `#[serde(default, deny_unknown_fields)]`; both `Option` fields are
  `skip_serializing_if = "Option::is_none"`, so defaults never invent a key.
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
§3.4). `init_with(enabled, dsn) -> Guard` installs the `tracing` subscriber and returns the
`ClientInitGuard`, held for the life of `main` so queued events flush on drop; `capture(&anyhow::Error)`
reports a command failure before it is printed.

Gates, in order: `telemetry` feature compiled (**opt-in since v1.0**, `default = []`) →
`telemetry = true` in the resolved config → a DSN in `APC_SENTRY_DSN`/`SENTRY_DSN`. `main` decides
*before* running the command so that a config that fails to load is itself reportable; `apc init` has
no config yet, so it falls back to the env variable alone.

The `tracing` subscriber is installed **unconditionally** (`fmt` layer to stderr, level from
`APC_LOG`, default `warn`), so local logs exist with no DSN and no network. Only when both gates hold
is the `sentry-tracing` layer attached, filtering events to `warn`/`error`; Sentry also receives
explicit command errors and, in debug builds, panics (`panic = "abort"` disables the hook in release).
`apc-agent` emits the first real events: `tool call` (debug, name only), `unknown tool` / `tool failed`
(warn), `agent stopped without a final answer` (warn).

Rules: no DSN shipped, `APC_TELEMETRY=0` beats any DSN, no usage metrics
(`auto_session_tracking: false`), never file/prompt/completion content. Libraries do not initialise a
client; only `apc-cli` does.

## Post-MVP (do not build during an MVP task)

SearXNG + Obscura plugins (v1.2), SQLite/`sqlite-vec` long-term memory (v1.3), `apc serve` + Tauri
(v2.0), Landlock/Seatbelt or WASM sandboxing (v2.1), dynamic plugin loading (v1.2).
