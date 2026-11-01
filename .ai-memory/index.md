# `.ai-memory/` — aplix code (`apc`)

Entry point for every agent session on this repo. Read this file first, then load only the
document that matches the task.

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

## Files

| File | Contents |
| :--- | :--- |
| `architecture.md` | Planned crate layout, dependency direction, key traits |
| `commands.md` | Cargo commands, tooling, verification steps |
| `gotchas.md` | Things that bite: edition/MSRV, async traits, README vs reality |
| `research.md` | Docs verified via Context7 (Cargo workspaces, async-trait, genai) |
| `workflows.md` | How to work here: task tracking (kaneo), roadmap order |

## Hard rules

- **The repo is currently docs-only.** `README.md`, `LICENSE`, `AGENTS.md`, `.ai-memory/` — no
  `Cargo.toml`, no `crates/`, no CI, no tests. Anything in the README that looks like a command or a
  file is a *target*, not a fact. Check the tree before running or editing anything.
- Do not add a provider/backend-specific dependency to `apc-agent` or `apc-cli`; swappable behaviour
  goes behind a trait in `apc-traits`.
- Every shell command executed by the agent must pass the Permission Guard.
- No telemetry, no analytics, no phone-home. Ever.
- Config precedence: defaults < user < project < local < env < flags. Config file: `apc.toml`.

## Conventions

- Naming: package `aplix-code`, binary/CLI `apc`, crates `apc-*`.
- Docs and code comments in English (the owner's global agent rules are French, but this repo is
  English — see `AGENTS.md`).
- `CONTRIBUTING.md` is referenced by the README but does not exist yet.
