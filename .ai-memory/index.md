# `.ai-memory/` — aplix code (`apc`)

Entry point for every agent session on this repo. Read this file first, then load only the document
that matches the task.

## What this project is

`aplix code` (binary **`apc`**) — a local-first, plugin-driven coding agent for the terminal,
written in Rust. MIT, community project, no affiliation with OpenAI/Anthropic.
Repo: `git@github.com:STIFLEUR390/aplix-code.git`, branch `main`.

Three principles that constrain every design decision:

1. **Local-first micro-decisions** — "is this command dangerous / what does the user want / is this
   context relevant" answered locally in <20 ms, free, offline. Shipped answer today is the built-in
   heuristic guard; `open-jev` (quantized DeBERTa-v3 over a local sidecar) is the target, behind
   `--features open-jev`, with the heuristics kept as the permanent fallback.
2. **Everything is a plugin** — model adapter, tools, memory backend, decision engine each behind a
   trait in `apc-traits`.
3. **Lite by design** — CLI binary < 15 MB; models downloaded on demand (resumable + SHA-256
   verified), never bundled.

## Sources of truth

| Order | Source | Authority |
| :-: | :--- | :--- |
| 1 | `SPEC.md` | the CLI MVP: commands + priorities, traits, stack, decisions. §8 = deliberate divergences from the README, with reasons. |
| 2 | `README.md` | product vision, positioning, post-MVP roadmap |
| 3 | this directory | accumulated knowledge: architecture, commands, gotchas, research |

If `SPEC.md` and the README disagree on an implementation detail, `SPEC.md` wins. If a task asks
for the README's version of a §8 decision, say why it was changed instead of silently reverting it.

## Files

| File | Contents |
| :--- | :--- |
| `architecture.md` | Crate layout, dependency direction, the five traits |
| `commands.md` | Cargo commands, release profile, tooling, docs lookup |
| `gotchas.md` | Things that bite: edition/MSRV, async traits, spec-vs-README traps |
| `research.md` | Docs verified via Context7 (Cargo workspaces, async-trait, reqwest, sentry) |
| `workflows.md` | P0/P1 priorities, roadmap order, task tracking (kaneo) |

## Hard rules

- **v1.0 is in place.** Cargo workspace at `crates/*` (9 crates, `resolver = "2"`, edition 2021,
  MSRV 1.80), every crate holds code: config, traits, HTTP adapters, JSONL sessions, heuristic guard
  + `open-jev` sidecar client, the resumable model store, file/shell tools, the agent loop, the tool
  registry and the `apc` binary; local `tracing` logs are always on, Sentry is compiled only with `--features telemetry`. CI runs
  fmt, clippy and tests in three feature shapes, and tags build Linux/macOS/Windows artifacts under
  a 15 MB size gate. `apc model` works but its catalog is empty on purpose (no upstream
  artifact, SPEC §8.12). Still targets, not code: hooks, context compaction, intent classification.
  Check `crates/<crate>/src` before trusting a doc claim.
- Do not add a provider/backend-specific dependency to `apc-agent` or `apc-cli`; swappable behaviour
  goes behind a trait in `apc-traits`.
- Every `run_shell` passes the Permission Guard (`P > 0.8` ⇒ confirmation). No bypass.
- The heuristic Permission Guard ships **first**; the neural model is optional (feature-gated).
- No tool runs a shell outside `RunShell`; it is the single audited path (v0.3).
- No telemetry, no analytics, no phone-home — one exception: opt-in Sentry error reporting,
  disabled by default, no DSN shipped, never user content (SPEC §3.4, §8.9).
- `#![forbid(unsafe_code)]` everywhere, no exception in V1.
- Config precedence: defaults < user < project < local < env < flags. File: `apc.toml`. API keys
  from the environment only.

## Conventions

- Naming: package `aplix-code`, binary/CLI `apc`, crates `apc-*`.
- `SPEC.md` is written in French (source document); code, comments and `AGENTS.md`/`.ai-memory/` in
  English.
- `CONTRIBUTING.md` is referenced by the README but does not exist yet.
