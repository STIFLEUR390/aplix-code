# Workflows

## Starting a session

1. Read `index.md`, then the one doc matching the task.
2. Check the tree: v0.1 is implemented, later milestones are not. Don't trust a doc claim about a
   module without looking at `crates/<crate>/src`.
3. For an MVP task, open `SPEC.md` at the relevant section. When the request contradicts a §8
   decision, say so instead of reverting silently.

## Roadmap order — build in this order

| Version | Milestone |
| :--- | :--- |
| v0.1 | Workspace skeleton, `apc init`, config resolution, `apc config` — **done** |
| v0.2 | `apc run` + `apc chat` (plain REPL), streaming over direct HTTP adapters — **done** (no `genai`, SPEC §8.10) |
| v0.3 | Permission Guard: heuristic, then `open-jev` + fallback |
| v0.4 | `apc model` + resumable downloads |
| v0.5 | Opt-in telemetry: `tracing` logs + Sentry, off by default |
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

Nine planned tasks exist (assigned to DEV MASTER, chained by `blocks` relations), plus the release
cleanup task added after telemetry was switched on for development. Update status/tickets through
these IDs instead of creating duplicates:

| # | Title | Due | Priority | taskId |
| :-: | :--- | :--- | :--- | :--- |
| 1 | v0.1 workspace + init/config | 2026-11-08 | high | `jj90o5xa0x4igxpd7rfjeal4` |
| 2 | v0.2 run/chat streaming | 2026-11-22 | high | `jzz8xyz22raiiwau7e7m4jzk` |
| 3 | v0.2b session memory JSONL | 2026-11-22 | medium | `sp7r39ab6iu9omkgz7znzomc` |
| 4 | v0.3 Permission Guard heuristic | 2026-12-06 | urgent | `ehjgpk8kkekz1qecoo2eql1j` |
| 5 | v0.3b open-jev feature flag | 2026-12-20 | medium | `p5hiqzv2bksz3r4ys6m3cr3u` |
| 6 | v0.4 model download | 2027-01-10 | high | `gsrte5nb2okub7z9szgshjzi` |
| 7 | v0.5 telemetry opt-in | 2027-01-24 | medium | `fs09rasodfc7bm4t1rf03kqu` |
| 8 | v1.0 MVP release | 2027-02-01 | high | `i8hdfhjqxqjqh70o4zrqz239` |
| 9 | v1.1 TUI/intent/skills | 2027-03-01 | low | `b2jrvx1yailriivbasw1k8uv` |
| 10 | v1.0 production DSN + telemetry off | 2027-02-01 | high | `ykajrzj02vow3xnfvd9s1rv1` |

Labels: `mvp`, `security`, `telemetry`, `release`, `post-mvp`.

Task 10 (created 2026-11-01, `blocks` task 8) exists because the dev phase compiles Sentry in by
default and arms it from the local `.env`: before shipping, the DSN moves to production and
`default = []` removes the crate again.

v0.1 (`jj90o5xa0x4igxpd7rfjeal4`) is implemented and sits in **In Review** — move it to Done once
the owner has looked at the diff.

## Before declaring work done

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`.
- Re-check the constraints this repo is judged on: no unsafe, no default-on telemetry (Sentry is
  opt-in and inert without a DSN), no provider-specific dep
  in `apc-agent`/`apc-cli`, release binary still under 15 MB (`ls -lh target/release/apc`),
  Permission Guard reachable from every shell execution path.
- If you learned something durable (a pin, a workaround, a doc fact), write it into `.ai-memory/`
  instead of leaving it in the chat.
