# Workflows

## Starting a session

1. Read `index.md`, then the one doc matching the task.
2. Check the tree: milestones up to v0.3 are implemented, later ones are not. Don't trust a doc claim about a
   module without looking at `crates/<crate>/src`.
3. For an MVP task, open `SPEC.md` at the relevant section. When the request contradicts a §8
   decision, say so instead of reverting silently.

## Roadmap order — build in this order

| Version | Milestone |
| :--- | :--- |
| v0.1 | Workspace skeleton, `apc init`, config resolution, `apc config` — **done** |
| v0.2 | `apc run` + `apc chat` (plain REPL), streaming over direct HTTP adapters — **done** (no `genai`, SPEC §8.10) |
| v0.2b | Session memory JSONL + `apc chat --continue` — **done** |
| v0.3 | Agent loop + built-in tools + heuristic Permission Guard — **done** |
| v0.3b | `open-jev` client for a local-jev sidecar behind a feature flag, `Cascade` fallback — **done** |
| v0.4 | `apc model` + resumable downloads — **done** (empty catalog, SPEC §8.12) |
| v0.5 | Opt-in telemetry: `tracing` logs + Sentry, off by default — **done** |
| v1.0 | MVP: CI (`fmt`, clippy ×3 shapes, tests, size gate) + tagged releases Linux/macOS/Windows — **done** ; télémetrie compilée uniquement sur `--features telemetry` — **done** |
| v1.1 | TUI `ratatui` (transcript + streaming + guard in the footer), classification d'intention heuristique, `skills/` chargés comme prompt système — **done** |
| v1.2 | Outils web : `web_search` (SearXNG) + `fetch_page` (HTTP simple + `rs-trafilatura`, pas Obscura — SPEC §8.13) — **done** ; chargement dynamique de plugins reste hors périmètre (SPEC §8.7) |
| v1.2b | Décider si `rig` remplace les adapters + la boucle d'agent — kaneo `d6k5r9tql0itnp3c82l5vzfs` (due 2027-04-12) — **done : refusé** (SPEC §8.2) |
| v1.3 | Long-term memory (SQLite + FTS5 / `sqlite-vec`) — kaneo `o47602h7odoky3hbjckrvrr9` (due 2027-04-26) — **done** : SQLite + FTS5, JSONL importé (SPEC §8.14) |
| v2.0 | `apc serve` (axum) + client Tauri — kaneo `rfqlho1w48tayi2h8n0u4oey` (due 2027-05-24) — **fait** (§8.15) : serveur + `crates/apc-desktop` hors workspace |
| v2.1 | Advanced sandboxing (landlock/seatbelt) or WASM plugins — kaneo `wkjodf93o65nnbou214pform` (due 2027-06-14) |

Chaining: v1.1 blocks v1.2, which blocks v1.2b, which blocks v1.3, which blocks v2.0, which blocks
v2.1 (kaneo `blocks` relations).

A request that jumps to a later milestone usually means: build the earlier foundation first, or scope
the change so it doesn't depend on what does not exist yet.

## Ordering constraints inside a feature

- **Permission Guard before the shell tool is reachable.** `run_shell` must be *callable only*
  through the guard. The heuristic ranks above the neural model: it is what makes v0.3 shippable
  with no model on disk.
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
| 4 | v0.3 agent loop + tools + guard heuristic | 2026-12-06 | urgent | `ehjgpk8kkekz1qecoo2eql1j` |
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
