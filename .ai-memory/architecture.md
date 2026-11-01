# Architecture (planned — README §Architecture)

Nothing here exists in the tree yet; this is the contract to implement against.

## Crate layout

```
aplix-code/
├── crates/
│   ├── apc-core/       # shared types, errors, config. No internal deps.
│   ├── apc-traits/     # all traits: Model, Tool, Memory, Decision, Plugin
│   ├── apc-agent/      # agent loop. Orchestration only.
│   ├── apc-tools/      # built-in tools (fs, shell)
│   ├── apc-decision/   # local decision engine (open-jev) + heuristics fallback
│   ├── apc-models/     # LLM provider adapters (via genai)
│   ├── apc-memory/     # session & long-term memory backends
│   ├── apc-plugins/    # plugin discovery & loading
│   └── apc-cli/        # the `apc` binary
└── skills/             # Markdown-based skill definitions (does not exist yet)
```

Dependency direction: `apc-core` at the bottom with no internal deps; `apc-traits` depends on
`apc-core`; every plugin crate depends on both; `apc-agent` orchestrates through traits only;
`apc-cli` wires concrete implementations together. `apc-agent` and `apc-cli` must stay free of
provider- or backend-specific dependencies.

## Key traits (README, simplified)

```rust
#[async_trait]
pub trait ModelAdapter: Send + Sync {
    async fn chat(&self, messages: &[Message], tools: &[Tool]) -> Result<Response>;
}

#[async_trait]
pub trait DecisionProvider: Send + Sync {
    async fn noul(&self, state: &str, question: &str) -> Result<f64>;
    async fn choice(&self, state: &str, question: &str, options: &[&str]) -> Result<(String, f64)>;
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult>;
}

pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn init(&self, ctx: &mut PluginContext) -> Result<()>;
}
```

`noul` returns a confidence score for a yes/no question about `state`; `choice` picks one of the
given options plus its confidence. Keep the method names exactly as written (`noul` is unusual but
comes from the README/local-jev lineage).

## Built-in tools (MVP)

`read_file`, `write_file`, `list_dir`, `run_shell`.

## CLI surface (MVP)

- `apc run "<prompt>"` — one-shot agent mode
- `apc chat` — interactive REPL; `apc chat --continue` resumes a session
- `apc init` — writes a commented `apc.toml`
- `apc model list|download|status|verify|remove` — local decision models (`open-jev`)
- `apc serve` — local HTTP server for editor integrations (post-MVP, v2.0)
- Tauri desktop app is a thin client over `apc serve` (post-MVP)

## Model download

`apc model download open-jev` → ~350 MB, resumable, SHA-256 verified. Same command again resumes
after an interruption and never leaves a corrupted file in place.

## Providers

Through the `genai` crate: OpenAI, Anthropic, Gemini, Ollama, DeepSeek, OpenRouter, Groq, and more.
Keys come from env (e.g. `ANTHROPIC_API_KEY`).

## Post-MVP (do not build during an MVP task)

`ratatui` TUI, SQLite + `sqlite-vec` RAG memory, SearXNG search, Obscura extraction, dynamic plugin
loading (native `.so` / WASM), `apc serve`, Tauri app, Landlock/Seatbelt sandboxing.
