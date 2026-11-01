# Architecture (SPEC §4)

Nothing here exists in the tree yet; this is the contract to implement against. `SPEC.md` is the
reference; `README.md` §Architecture is the same layout minus the V1 refinements below.

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

| Command | Priority |
| :--- | :--- |
| `apc run "<prompt>"` | P0 |
| `apc chat` (plain REPL; `--continue` resumes) | P0 |
| `apc init` | P0 |
| `apc model list\|download\|status\|verify\|remove` | P0 |
| `apc --version` (also prints the loaded decision model) | P0 |
| `apc config` (effective config + which layer set it) | P1 |

## Providers

Through `genai`: OpenAI, Anthropic, Gemini, Ollama, DeepSeek, OpenRouter, Groq. Provider + model in
`apc.toml`; keys from env (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …).

## Session memory

JSONL under `${XDG_DATA_HOME:-~/.local/share}/aplix/sessions/`, one file per session. `apc chat
--continue` reloads the latest. No long-term memory in V1 (v1.3).

## Post-MVP (do not build during an MVP task)

`ratatui` TUI (v1.1), intent classification (v1.1), `skills/` (v1.1), SearXNG + Obscura plugins
(v1.2), SQLite/`sqlite-vec` long-term memory (v1.3), `apc serve` + Tauri (v2.0), Landlock/Seatbelt
or WASM sandboxing (v2.1), dynamic plugin loading (v1.2).
