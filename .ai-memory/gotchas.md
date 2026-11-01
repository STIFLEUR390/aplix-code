# Gotchas

## Repository reality vs docs

- `README.md` (vision) and `SPEC.md` (MVP) describe the **target**. v0.1 code exists (workspace +
  config + `apc init`/`config`); `apc run`/`chat`/`model`, all traits and the other seven crates do
  not. Still absent: CI, `skills/`, `CONTRIBUTING.md`, `.github/ISSUE_TEMPLATE/`.
- Coding tasks name a crate that may still be a placeholder — check its `src/lib.rs` before assuming
  a module exists.

## Config resolution (v0.1)

- `apc-core::config` is the only place that knows the precedence order; the CLI just builds `Layer`s
  (files, then env, then flags) and calls `resolve`.
- `TEMPLATE` is both the file written by `apc init` and the documented key list. `template_matches_the_defaults`
  breaks the moment the two drift — that test is the guard, not a formality.
- TOML scoping bit once already: a `telemetry = false` placed after `[decision]` becomes
  `decision.telemetry` and fails to deserialize. Top-level keys stay above the first `[table]`.
- `deny_unknown_fields` on `Config`/sections is intentional: `[provder]` must be an error. If a key is
  ever removed, users get a hard failure — that is the trade-off chosen, not an oversight.
- `Layer::validate` re-deserializes each layer so a bad value names the file it came from. Env and
  flag layers go through the same path, labelled `environment` / `command line`.
- Empty `APC_*` values are ignored (treated as unset). `APC_TELEMETRY` accepts
  `1/0/true/false/yes/no/on/off`, not just TOML booleans.

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

## Docs drift

- `README.md`'s trait sketch is *illustrative*: it omits `MemoryStore` and its `Tool` has no
  `schema()`. `SPEC.md` §4 is the contract — five traits, streaming `chat`, tool schema required
  by `genai` tool calling.
- README tables were realigned with SPEC §7 (v0.5 telemetry, v1.3, v2.1) and with §8.3 (streaming).
  If they diverge again, SPEC wins; fix the README rather than the spec.

## Tooling: REA is not applicable here

- `rea` reverse-engineers **shipped artifacts** (native binaries, Electron/JS bundles, firmware,
  managed assemblies, browser pages). This repo contains only Markdown — no binary, no build output,
  no capture. Its own guidance says to skip it for ordinary source-repository analysis.
- Reconsider only when there is something to take apart: e.g. analysing a competitor's release
  binary, or reconstructing a shipped `apc` artifact whose source you do not have.

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

- `aplix-code` = repo name, not a crate. Binary crate `apc-cli`, bin name `apc`
  (`cargo install --path crates/apc-cli`); the README's `cargo install aplix-code` is a v1.0 target.
- Config file is `apc.toml` (not `aplix.toml`, not `.apcrc`).
