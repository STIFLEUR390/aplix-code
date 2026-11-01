# Workflows

## Starting a session

1. Read `index.md`, then the one doc matching the task.
2. `ls` / `find` the tree — confirm whether the workspace has been scaffolded yet. `AGENTS.md` and
   `index.md` describe the documentation-only state; update them the moment that changes.
3. For an MVP task, open `SPEC.md` at the relevant section. When the request contradicts a §8
   decision, say so instead of reverting silently.

## Roadmap order — build in this order

| Version | Milestone |
| :--- | :--- |
| v0.1 | Workspace skeleton, `apc init`, config resolution, `apc config` |
| v0.2 | `apc run` + `apc chat` (plain REPL) via `genai`, streaming |
| v0.3 | Permission Guard: heuristic, then `open-jev` + fallback |
| v0.4 | `apc model` + resumable downloads |
| v1.0 | MVP: stable CLI, docs, CI, releases Linux/macOS/Windows |
| v1.1 | `ratatui` TUI, intent classification, `skills/` |
| v1.2 | SearXNG + Obscura plugins, dynamic plugin loading |
| v1.3 | Long-term memory (SQLite + FTS5 / `sqlite-vec`) |
| v2.0 | `apc serve` + Tauri |
| v2.1 | Advanced sandboxing (landlock/seatbelt) or WASM plugins |

A request that jumps to a later milestone usually means: build the earlier foundation first, or scope
the change so it doesn't depend on what does not exist yet.

## Ordering constraints inside a feature

- **Permission Guard before the shell tool is reachable.** `run_shell` must be *callable only*
  through the guard. The heuristic ranks above the neural model: it is what makes v0.3 shippable
  before v0.4 exists.
- **Own types before adapters.** `apc-core` types → `apc-traits` → `apc-models` adapter → `apc-agent`
  loop. Never let a `genai` type leak past `apc-models`.
- **P0 before P1.** `apc config` (P1) never blocks `apc run`/`chat`/`init`/`model` (P0); intent
  classification is v1.1.

## Task tracking

kaneo MCP, fixed IDs:

- workspace `GAiYlKA3Nr2yEh3Ea40IY0f0tvK4rjQ0`
- project `f0yyfbfaoe4610r2d2hc8qir`

## Before declaring work done

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`
  (once they exist).
- Re-check the constraints this repo is judged on: no unsafe, no telemetry, no provider-specific dep
  in `apc-agent`/`apc-cli`, release binary still under 15 MB (`ls -lh target/release/apc`),
  Permission Guard reachable from every shell execution path.
- If you learned something durable (a pin, a workaround, a doc fact), write it into `.ai-memory/`
  instead of leaving it in the chat.
