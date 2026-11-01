# Workflows

## Starting a session

1. Read `index.md`, then the one doc matching the task.
2. `ls` / `find` the tree — confirm whether the workspace has been scaffolded yet. `AGENTS.md` and
   `index.md` describe the docs-only state; update them the moment that changes.

## Roadmap order — build in this order, don't skip ahead

| Version | Milestone |
| :--- | :--- |
| v0.1 | Skeleton, workspace, `apc init` + config resolution |
| v0.2 | `apc run` + `apc chat` via `genai` (OpenAI/Anthropic) |
| v0.3 | Permission Guard with `open-jev` + fallback heuristics |
| v0.4 | `apc model` subcommand + resumable downloads |
| v1.0 | MVP: stable CLI, docs, CI, releases Linux/macOS/Windows |
| v1.1 | `ratatui` TUI, intent classification |
| v1.2 | SearXNG + Obscura plugins |
| v2.0 | `apc serve` + Tauri desktop app |

A request that jumps to a later milestone usually means: build the earlier foundation first, or scope
the change so it doesn't depend on what does not exist yet.

## Ordering constraint inside any agent feature

Permission Guard is not optional plumbing: `run_shell` must be *callable only* through it. Build the
trait in `apc-traits` and the heuristic fallback before wiring the tool, so the agent never has a
path to execute a command unchecked.

## Task tracking

kaneo MCP, fixed IDs:

- workspace `GAiYlKA3Nr2yEh3Ea40IY0f0tvK4rjQ0`
- project `f0yyfbfaoe4610r2d2hc8qir`

## Before declaring work done

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`
  (once they exist).
- Check the constraint list that this repo is judged on: no unsafe in user-facing code, no
  telemetry, no provider-specific dep in `apc-agent`/`apc-cli`, release binary still under 15 MB
  (`ls -lh target/release/apc`).
- If you learned something durable (a pin, a workaround, a doc fact), write it into `.ai-memory/`
  instead of leaving it in the chat.
