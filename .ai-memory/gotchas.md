# Gotchas

## Repository reality vs docs

- `README.md` (vision) and `SPEC.md` (MVP) describe the **target**. The tree is documentation only:
  no `Cargo.toml`, no `crates/`, no CI, no tests, no `skills/`, no `CONTRIBUTING.md`, no
  `.github/ISSUE_TEMPLATE/`. Never run `cargo build` "to check" without looking at the tree first.
- Any task naming an `apc-*` crate means *create* it, not edit it.
- Git history is README edits only — no code history to learn from.

## Spec vs README: traps

`SPEC.md` §8 lists decisions that deliberately contradict the README. Reverting one without reading
the reason is the most likely regression here:

| Topic | Wrong (README / first draft) | Right (SPEC §8) |
| :--- | :--- | :--- |
| `apc chat` UI | rich TUI | plain stdin/stdout REPL until v1.1 |
| Streaming | full response only | stream in `ModelAdapter::chat` from V1 |
| Agent abstractions | `rig-core` + `genai` | own types in `apc-core`, adapt `genai` (§8.2) |
| Model download | `http-ferry` | `reqwest` + `sha2`, hand-rolled `Range` resume (§8.4) |
| Release opt level | `opt-level = "z"` | `opt-level = 3` + strip; size budget is met by stripping (§8.5) |
| Decision model | required dependency | feature flag; heuristic is always present (§8.1) |
| `unsafe` | exception for native plugins | none in V1, nothing is loaded dynamically (§8.8) |
| Telemetry | forbidden outright | opt-in Sentry error reporting, off by default, no DSN shipped (§3.4, §8.9) |

## Rust edition / MSRV

- README claims Rust **1.80+**. Edition 2024 requires **1.85+**, so use **edition 2021** and
  `resolver = "2"` unless the MSRV is deliberately raised.
- `rust-version = "1.80"` in `[workspace.package]` is what keeps `cargo install` honest.

## Async traits

- Native `async fn` in traits (stable since 1.75) is **not dyn-compatible**. Since "everything is a
  plugin" means `Box<dyn ModelAdapter>` / `Arc<dyn Tool>`, the `#[async_trait]` attribute from
  `dtolnay/async-trait` is required — that is why the trait sketches show it.
- Streaming makes it worse: the returned `ChatStream` must be a boxed `Stream` (or a concrete enum)
  because a trait method cannot return an anonymous `impl Stream` through `dyn`.

## Binary size budget (< 15 MB)

- Easiest constraint to break. Avoid `openssl-sys` (prefer `rustls`), avoid bundling a tokenizer or
  an inference runtime, keep the release profile in `commands.md`.
- Any dependency that links an ML runtime (candle/ort) blows the budget — that is exactly why the
  decision model is downloaded at runtime and feature-gated.

## Permission Guard

- The heuristic must exist before the neural model; otherwise the first working build has no safety
  net and the model is not even downloadable yet (v0.3 precedes v0.4).
- `P > 0.8` ⇒ confirm. A "bypass everything" flag is not in the MVP; if one is ever added it must be
  explicit, logged, and visible in `apc config`.

## Model downloads and sessions

- `~350 MB` for `open-jev`; the download must be resumable (`Range`) and SHA-256 verified, writing to
  `.part` and renaming only after the digest matches. A Ctrl-C must never leave a file that later
  loads silently.
- Never hardcode `~/.local/share` — use `XDG_DATA_HOME` (crate `directories`) so macOS/Windows work.

## Task tracking (kaneo MCP)

- Use workspace `GAiYlKA3Nr2yEh3Ea40IY0f0tvK4rjQ0` and project `f0yyfbfaoe4610r2d2hc8qir`.
  Don't create a parallel workspace/project.

## Telemetry / Sentry

- Never call `sentry::init(())` — it applies library defaults that read ambient env (`SENTRY_DSN`),
  so reporting could switch on behind the user's back. Build `ClientOptions::new()` and set `dsn`
  explicitly only after `telemetry = true` in `apc.toml` **and** a DSN was resolved.
- Nothing is sent without a DSN: `ClientOptions::new()` has `dsn = None` and the client is entirely
  disabled (`is_enabled() == false`). That is the opt-out, not a `cfg!(test)` hack.
- Keep the `ClientInitGuard` alive for the whole process (`let _guard = ...`) — dropping it flushes
  and shuts the transport down.
- Default event filter: `EventFilter::Event` for `ERROR`, `Ignore` otherwise, so prompts and file
  contents (which are `debug`/`info` chatter) never leave the machine.

## Naming

- `aplix-code` = package/repo, `apc` = binary. `cargo install aplix-code` produces `apc`.
- Config file is `apc.toml` (not `aplix.toml`, not `.apcrc`).
