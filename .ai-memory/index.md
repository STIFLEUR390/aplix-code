# `.ai-memory/` — aplix code (`apc`)

Entry point for every agent session on this repo. Read this file first, then load only the document
that matches the task.

## What this project is

`aplix code` (binary **`apc`**) — a local-first, plugin-driven coding agent for the terminal,
written in Rust. MIT, community project, no affiliation with OpenAI/Anthropic.
Repo: `git@github.com:STIFLEUR390/aplix-code.git`, branch `main`.

Three principles that constrain every design decision:

1. **Local-first micro-decisions** — "is this command dangerous / what does the user want / is this
   context relevant" answered by a local model (`open-jev`, quantized DeBERTa-v3) in <20 ms, free,
   offline. The cloud LLM is only called for actual generation.
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
| `research.md` | Docs verified via Context7 (Cargo workspaces, async-trait, genai) |
| `workflows.md` | P0/P1 priorities, roadmap order, task tracking (kaneo) |

## Hard rules

- **The repo is documentation only.** `README.md`, `SPEC.md`, `LICENSE`, `AGENTS.md`,
  `.ai-memory/` — no `Cargo.toml`, no `crates/`, no CI, no tests. Anything that looks like a command
  or a file in the docs is a *target*, not a fact. Check the tree before running or editing.
- Do not add a provider/backend-specific dependency to `apc-agent` or `apc-cli`; swappable behaviour
  goes behind a trait in `apc-traits`.
- Every `run_shell` passes the Permission Guard (`P > 0.8` ⇒ confirmation). No bypass.
- The heuristic Permission Guard ships **first**; the neural model is optional (feature-gated).
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
