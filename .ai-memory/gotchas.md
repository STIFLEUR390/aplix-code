# Gotchas

## Repository reality vs docs

- `README.md` (vision) and `SPEC.md` (MVP) describe the **target**. v0.4 code exists (workspace,
  config, `apc init`/`config`, `apc run`/`chat`, JSONL sessions, tools + guard, `open-jev` client,
  model store, Sentry telemetry). Still absent: CI, `skills/`, hooks, `CONTRIBUTING.md`,
  `.github/ISSUE_TEMPLATE/`.
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

## Providers and streaming (v0.2)

- `genai` is **not in this machine's cargo cache** and crates.io delivers ~4-7 KB/s, so it cannot be
  added. Adapters are hand-written on `reqwest` (SPEC §8.10). Do not "restore" genai in a task
  without checking `ls ~/.cargo/registry/src/*/ | grep genai` first.
- `provider.base_url` is the **version directory**: the adapter appends `/chat/completions` (or
  `/messages`). `http://127.0.0.1:31415` without `/v1` made the local relay return its own HTML page
  — streaming nothing, exit code 0, until the content-type guard was added.
- That guard is load-bearing: a `200` whose `content-type` is not `text/event-stream` becomes
  `"{base_url}: expected an event stream, got `{ct}`: {first 200 chars}"`. Keep it.
- The local relay at `127.0.0.1:31415` (`model = "auto"`) is a router in front of paid providers: it
  answers `502 upstream` with `rate_limited ×1, timeout ×1` under load, and the first `apc chat`
  attempt hung until the harness 120 s timeout. That is upstream, not a bug in `apc-models` — check
  `curl -s -H "Authorization: Bearer $APC_API_KEY" http://127.0.0.1:31415/v1/models` before debugging
  the client.
- Keys resolve as `APC_API_KEY` first, then the provider's own variable; ollama needs none. They are
  read in `build()` and never stored in config (`APC_API_KEY` is intentionally absent from `ENV_KEYS`).
- Anthropic needs `max_tokens` (4096) and its system prompt at the top level; the OpenAI dialect keeps
  `system` as a message. That asymmetry lives in `body()` — don't unify it.
- `stream::unfold` state must be `Option<…>` for termination, and the closure needs `move` to capture
  the dialect. Both were compile errors before they were learned.
- CLI tests fake the network with a one-shot `TcpListener` + canned SSE body; never point a test at
  the real relay.

## Session memory (v0.2b)

- Files are append-only JSONL, one `Message` per line. Never rewrite a session file in place: a
  reader (`load`) tolerates only the *last* line being torn, and silently skips any unparsable line.
- Session ids are `{:013}-{:06}` unix seconds+microseconds — swapping to UUIDs would break the
  "latest = sort by name" rule that `latest_session()` relies on.
- Two sessions created in the same microsecond would collide; the ordering assumption is worth more
  than a UUID, and a collision only merges two bots' turns.
- `load(None)` means latest, so a caller that wants "start fresh" must call `new_session()`, not
  `load(None)`.
- `apc chat` writes the user turn before the request: a provider failure still leaves the prompt in
  the file, and a retry via `--continue` continues from it.
- Tests: `cargo test -p apc-memory`; the CLI test points `XDG_DATA_HOME` at its sandbox
  (`dir/data/aplix/sessions`), so runs never touch your real history.

## Model store (v0.4)

- **The catalog is empty and that is correct.** `local-jev` publishes no weights, so no artifact
  exists to download (SPEC §8.11). `apc model download open-jev` answers `unknown model `open-jev``.
  Do not "fix" it by adding a URL and a made-up SHA-256: the download would then fail verification
  or install something unverified. A real artifact is one `ModelEntry` in `CATALOG`.
- A `.part` that fails the checksum is **deleted**, not resumed: resuming a corrupt prefix can only
  produce another corrupt file.
- Dangerous assumption to re-check: a server that ignores `Range` and answers `200` must lead to a
  truncate-and-restart, never an append (covered by `a_server_that_ignores_the_range_restarts_from_zero`).
- `reqwest`/`sha2` are now unconditional in `apc-decision`, so `--no-default-features` no longer
  drops `reqwest`; only `apc-cli`'s Sentry goes away.
- **The resume path has never run against a real artifact.** It is proven by local stubs only
  (16-byte bodies, stubs that honour or ignore `Range`); no ~350 MB download, and no real Ctrl-C,
  has been exercised here — the network tops out at a few KB/s. Treat it as tested logic, not as a
  field-proven download, and re-run it end to end the day a catalog entry exists.

## Telemetry / Sentry (v0.1 dev phase)

- Two independent gates, both required: `telemetry = true` in the resolved config **and** a DSN in
  `APC_SENTRY_DSN`/`SENTRY_DSN`. Losing either silently disables everything; check `apc config`
  before assuming a missing event is a Sentry bug.
- `telemetry_enabled()` resolves the config **twice** (once to decide, once for the command) so a
  broken config still reports itself. That duplication is deliberate, ~0.1 ms.
- `sentry` is a **default** feature right now. Anything that must prove the feature-off path
  (`sentry` absent from the graph, no dead_code warning) has to build with
  `--no-default-features`. Feature-off items must stay behind `#[cfg(feature = "telemetry")]` or the
  build warns.
- `panic = "abort"` in the release profile means the Sentry panic hook has nothing to unwind: only
  explicitly captured errors leave a release binary.
- Never call `sentry::init(())` — the no-argument form reads ambient `SENTRY_DSN` and arms telemetry
  behind the config gate's back. `sentry::init((dsn, options))` keeps the gate explicit.
- `.env` is read only in debug builds, from the process's current directory, by `load_dotenv` in
  `crates/apc-cli/src/main.rs` (no dependency; `KEY=VALUE`, `#` comments, existing env wins). Release
  builds ignore it on purpose.
- `release-health` is off (`auto_session_tracking: false`): session tracking is usage metrics, which
  SPEC §9.8 rules out.

## Build environment (this machine, Nov 2026)

- crates.io downloads crawl at ~7 KB/s and `cargo add` / online `cargo build` hang on the index. Build
  with `--offline`; the registry cache already holds almost everything.
- `sentry 0.48.2` is cached but `sentry-anyhow` is not, so no `capture_anyhow` — use
  `sentry::capture_error(error.as_ref())` with an explicit
  `&(dyn std::error::Error + Send + Sync + 'static)` binding (plain `as_ref()` is ambiguous).
- `ureq 3.3.0` needs `der ^0.8.0`, which is yanked: an offline resolve dies there. Use sentry's
  `reqwest` + `rustls` features (reqwest 0.13.4 is cached) and skip the `transport` feature, which
  pulls `native-tls`.
- `webpki-roots 1.0.11` was missing from the cache and had to be fetched by hand into
  `~/.cargo/registry/cache/index.crates.io-*/`. If a build fails with "attempting to make an HTTP
  request, but --offline was specified", that is the fix.

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

## Permission Guard (v0.3)

- The heuristic exists; the neural model does not (v0.3b). `score()` in
  `crates/apc-decision/src/lib.rs` lowercases the command, substring-matches `PATTERNS`, and keeps the
  highest score — no parsing, no shell-awareness, so `echo 'rm -rf /'` is flagged too (a false
  positive the user can decline; the reverse would be a hole).
- `P > 0.8` ⇒ confirm, strict `>`: a pattern scoring exactly 0.8 never prompts under the default
  threshold. `git push --force` is 0.82 for that reason; if you add a 0.8 entry, you added nothing.
- Declining returns text to the model, and the command is never spawned — that is the contract
  (`a_refused_confirmation_does_not_run_the_command` asserts it with a `touch` marker).
- The guard lives in `RunShell`, not in `apc-cli` and not in `apc-agent`. Any future tool that spawns
  a process must take the same `DecisionProvider` + `Approver` pair, or the "no path executes a shell"
  rule breaks silently.
- A "bypass everything" flag is not in the MVP; if one is ever added it must be explicit, logged, and
  visible in `apc config`.

## Decision backend / open-jev (v0.3b)

- The model is **not embedded**: `crates/apc-decision/src/jev.rs` is an HTTP client for a
  `local-jev` sidecar (SPEC §8.11). No inference crate (`candle`, `ort`, `tract`) is cached on this
  machine and weights are absent, so an embedded backend could not even be compiled and tested here.
- `local-jev` v1.0.0 source sits at `/tmp/local-jev/Local-Jev-1.0.0` (and
  `~/Téléchargements/ABDM/Compressed/Local-Jev-1.0.0.zip`). It cannot be built here: it needs
  `axum 0.7`, not in the cargo cache. Port 8080 is Apache on this machine, so `http://127.0.0.1:8080`
  is *not* a local-jev instance.
- **Never take `confidence` as the risk.** It is the winner's probability; a confident `no` would
  read as a dangerous command. Read `probabilities` for the `yes` option. Test
  `noul_reads_the_yes_probability_not_the_confidence` pins this.
- local-jev's `noul` route classifies (option + category), it does not answer yes/no; that is why
  `DecisionProvider::noul` is emitted as a two-option `choice`.
- `Cascade` swallows the error on purpose: a dead sidecar must not surface to the user, only change
  who answers. Verified live by pointing `APC_DECISION_ENDPOINT` at a closed port: the command ran
  with no prompt.
- **Measured trap:** `cargo install local-jev` gives v2.0.0, whose `serve` has no weight flag and
  whose engine answers `0.5 / 0.5` to everything (`local-jev score --text "rm -rf /" --criteria …`
  → `0.5000`). Wired naively, the primary backend returned 0.5 for `rm -rf`, below the 0.8
  threshold, so the command ran with **no** confirmation — the guard was off while looking on.
  Fixed in `flat()`: a gap < 0.01 between the top two probabilities is an error, not a low score.
  Any new backend needs the same rule.
- **There is no ONNX model to download.** local-jev 2.0.0 has no inference dependency (no `ort`,
  no `candle`) and its `ModelBackbone::from_file` / `NeuralWeights` path is never called — dead code
  kept alive by its own unit tests. `decide_choice` is trigram/keyword affinity: `no`/`yes` ties at
  0.5 for everything, and descriptive options (`safe`/`destructive`) score "destructive" 1.0 even
  for `ls -la`. Don't go hunting for weights; a real neural decision needs a separate runtime.
- Real local-jev on this machine: `local-jev serve --port 8091` (8080 is Apache), then
  `APC_DECISION_ENDPOINT=http://127.0.0.1:8091` on a binary built with `--features open-jev`.

## Model downloads and sessions

- `~350 MB` for `open-jev`; the download must be resumable (`Range`) and SHA-256 verified, writing to
  `.part` and renaming only after the digest matches. A Ctrl-C must never leave a file that later
  loads silently.
- Never hardcode `~/.local/share` — use `XDG_DATA_HOME` (crate `directories`) so macOS/Windows work.

## Agent loop / tools (v0.3)

- Tool arguments arrive as a **string** (that is how both providers stream them). `apc-agent` parses
  once, and falls back to `{"raw": "…"}` when the model emits invalid JSON.
- Tool `execute` errors are answers, not crashes: they become the `role: tool` message. The user only
  sees a hard error for a provider failure or `MAX_STEPS` exhaustion.
- `apc-cli` persists `messages[before..]` after `Agent::run`; the agent appended the assistant turn and
  every tool exchange. Persisting inside the loop would need the store passed into `apc-agent`, which
  must stay I/O-free.
- `apc-tools::resolve` is lexical on purpose. `canonicalize` would fail for files that do not exist
  yet (`write_file`) and would make safety depend on the filesystem.
- OpenAI sends tool-call fragments indexed by `delta.tool_calls[].index`; Anthropic sends
  `content_block_start` + `input_json_delta`. Both are flushed as one `ChatEvent::ToolCall` — a
  server that closes the stream without `finish_reason`/`message_stop` still gets its calls from the
  `None` branch of `stream_from`.
- `Approver::confirm` blocks the async runtime on stdin. Fine for a one-command-at-a-time CLI; if
  concurrent tool calls appear, move it to `spawn_blocking`.

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
