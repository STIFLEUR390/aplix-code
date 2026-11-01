# Research log

## Reference projects (links verified 2026-11-01)

- `rig` — source <https://github.com/0xPlaygrounds/rig>, docs <https://rig.rs/docs>. **Not yet a
  dependency** (SPEC §8.2). Verified through Context7 (`/0xplaygrounds/rig`, 803 snippets) on
  2026-11-01, API 0.44+:
  - `Tool` trait: `const NAME`, `type Args: Deserialize`, `type Output`, `type Error`,
    `description()`, `parameters() -> Value` (JSON Schema), `annotations() -> ToolAnnotations`
    ("what calling the tool does to the world, for the host's policies (approval, plan mode,
    which calls run side by side)"), `footprint(&args) -> Touches`, `call(&self, &mut ToolContext,
    Args) -> impl Future`. `ToolDyn` is **gone** — runtime-named agents use `DynamicTool` /
    `Agent::into_tool()`.
  - `agent.runner(prompt).history(history).max_turns(3).run()` / `.stream()` replaces the removed
    `completion()` / `stream_completion()`.
  - `AgentHook` replaced `PromptHook`/`HookAction` (removed before 0.44): `on_run_start`,
    `on_completion_call` (patch the prompt or stop), `on_model_select` (synchronous, local),
    `on_run_settled`; streaming deltas are `TextDelta { delta, aggregated }`,
    `ReasoningDelta { part, delta, aggregated }`, `ToolCallDelta { part, tool_name, delta,
    aggregated }` — tool calls are *not* executed before their end.
  - Open question for the spike: whether `ToolAnnotations` can carry our risk threshold, or whether
    the guard has to sit in an `AgentHook`.
- `harness-rs` — <https://github.com/liliang-cn/harness-rs> (the "everything is a plugin" layout
  behind `apc-traits` + `apc-plugins`).
- `OpenCode` — <https://github.com/anomalyco/opencode> (server-first design target for `apc serve`).
- `local-jev` — <https://github.com/ronitgupta138/Local-Jev>.


Two tiers: **verified** (read from published docs via Context7, dated) and **unverified** (chosen by
`SPEC.md` or the candidate list, nothing read yet). Re-check with `npx ctx7@latest docs …` before
relying on an unverified entry.

## Verified — Cargo workspaces

- Root manifest: `[workspace] members = [...]`, `resolver = "2"` for edition-2021 workspaces
  (resolver "3" for edition 2024 / MSRV 1.84+).
- `[workspace.package]` centralises `version`, `edition`, `rust-version`, `license`; members inherit
  with `version.workspace = true`.
- `[workspace.dependencies]` + `serde = { workspace = true }` is the mechanism that keeps nine crates
  on one dependency version.
- A workspace has **one `Cargo.lock` at its root**, shared by all members
  (`default_lock_root` → `self.root()` in cargo's `workspace.rs`). No per-crate lockfiles.
- Edition-2024 members may override an inherited dependency's `default-features`; on edition 2021
  `default-features = false` on an inherited dep is **ignored with a warning** (RFC 3945). Declare
  feature choices on the `[workspace.dependencies]` entry.

## Verified — async-trait (`/dtolnay/async-trait`)

- Exists precisely because native async-fn-in-trait is not supported for `dyn Trait`. Required here
  because the plugin architecture stores trait objects.
- Cost: each async method is boxed into a `Pin<Box<dyn Future>>`. Fine for the agent loop; remember
  it if a hot path ever lives behind a trait object.

## Superseded — genai (`/websites/rs_genai`, latest 0.5.2)

Status: **not a dependency** (SPEC §8.10 — not in this machine's cache, and the HTTP surface needed is
`/chat/completions` + `/messages`). Kept only in case a future version reinstates it; do not read
these notes as describing current code.

- Client construction: `Client::builder()` → `ClientBuilder` with `with_reqwest`,
  `with_config(ClientConfig)`, `with_chat_options(ChatOptions)`, `with_auth_resolver`,
  `with_service_target_resolver`, `with_model_mapper`, `with_adapter_kind(AdapterKind)`, `build()`.
- `with_adapter_kind` binds a client to one provider and routes directly instead of inferring the
  adapter from the model name on every call — the right shape for one `ModelAdapter` per provider.
- Requests: `ChatRequest::from_user("weather")`, `.with_tools(vec![Tool::new("get_weather")])`,
  options `ChatOptions::default().with_tool_choice(ToolChoice::tool("get_weather"))`,
  `Client::exec_chat`.
- Tool plumbing: the model replies with `tool_calls`; results go back as a message with role `tool`,
  `content` = the result, `tool_call_id` = the call's `call_id` (genai: `ContentPart::ToolResponse`
  inside `ChatRole::Tool`).
- Strict tool mode: OpenAI requires `additionalProperties: false` on object schemas; genai adds it
  when `tool.strict == true`. Always provide a JSON-schema `parameters` value (`Tool::schema`).
- DeepSeek/Kimi require `reasoning_content` echoed back in the assistant message.
- Auth/target primitives: `AuthData::from_single("key")`, `ServiceTarget { model, auth, endpoint }`
  — the hook for Ollama / custom gateways.
- Streaming: `Client::exec_chat_stream(model, chat_req, options: Option<&ChatOptions>) ->
  Result<ChatStreamResponse>`. `ChatStreamResponse { stream: ChatStream, model_iden }`;
  `ChatStream` implements `Stream<Item = Result<ChatStreamEvent, Error>>` (drive it with
  `StreamExt`). Same `Into<ModelSpec>` argument as `exec_chat` (`&str`, `ModelIden`,
  `ServiceTarget`). So `ModelAdapter::chat` can box a `ChatStream` directly.

## Verified — sentry (`/websites/rs_sentry_sentry`)

- `sentry::Client::with_options(ClientOptions)`: **if the DSN is `None` the client is entirely
  disabled**; `Client::is_enabled()` returns `false` for `ClientOptions::new()`.
  That is the whole opt-out story — no DSN means no network, no background thread.
- `sentry::init(ClientOptions)` returns a guard that must stay alive; dropping it flushes/shuts down.
- Tracing integration: `sentry::integrations::tracing::layer()` (feature `tracing`), registered on
  the `tracing_subscriber` registry next to the fmt layer. Customise with
  `.event_filter(|md| match *md.level() { tracing::Level::ERROR => EventFilter::Event, _ =>
  EventFilter::Ignore })` and `.span_filter(...)`. An event **mapper** overrides the filter when both
  are set. `traces_sample_rate(f64)` enables spans/transactions; spans are off without it.

## Verified — reqwest, directories, clap, tokio

- **`reqwest`** (`/seanmonstar/reqwest`): `Response::bytes_stream()` (feature `stream`) →
  `impl Stream<Item = Result<Bytes>>`; `Response::chunk()` → next frame; `bytes()` collects.
  Resume is a `Range: bytes=<n>-` request header on the existing `.part` length — nothing special.
  Feature naming in 0.13: `json` = serde support, `stream` = `bytes_stream()`, `rustls` =
  `__rustls-aws-lc-rs` + `rustls-platform-verifier`; `default` pulls `default-tls` + `http2`.
- **`directories`** (`/websites/rs_directories`): `ProjectDirs::data_dir()` → Linux
  `$XDG_DATA_HOME` or `~/.local/share`, macOS `~/Library/Application Support`, Windows
  `{FOLDERID_RoamingAppData}\<project>\data`. Exactly the session path in SPEC §2.5.
- **`clap`** (`/clap-rs/clap`): `#[derive(Subcommand)]` on an enum registers the variants;
  `#[command(version, propagate_version = true)]` serves `--version` on the root **and** every
  subcommand.
- **`tokio`**: `/tokio-rs/tokio` (and `/websites/rs_tokio`, more snippets) — no API doubt for the
  runtime; `#[tokio::main]` in `apc-cli` only.
- Existence checked, no API read needed: `thiserror`, `anyhow`, `sha2`, `toml`, `serde_json`,
  `ratatui`.

## Verified — local-jev 2.0.0 (read from the crate source, not docs)

Context7's only match (`/qew7/jev-feels`) failed to fetch and is unrelated; the authority here is the
installed source at `~/.cargo/registry/src/*/local-jev-2.0.0/`. Findings, measured on a real
`local-jev serve --port 8091`:

- **HTTP contract** (`src/server.rs`, `src/domain.rs`): `POST {base}/v1/systemone/choice` with
  `{state, question, options: [String], temperature?: f32}` → `{choice, confidence, probabilities:
  [{option, probability, logit}], latency_ms, cached}`. Also `/score`, `/noul`, `/route`, `/health`,
  `/metrics`, `/v1/models`.
- `noul` is a **classification** (`{selected, category, confidence}`), not a yes/no probability, so
  `DecisionProvider::noul` is emitted as `choice(["no","yes"])`.
- **No inference engine.** Dependencies are axum/tokio/serde/clap/colored/tracing — no `ort`, no
  `candle`. `ModelBackbone::from_file` (`src/model.rs`, `NeuralWeights(PathBuf)`) is never called;
  it is dead code with its own unit tests. `serve` exposes no weights flag.
- **No discrimination.** `decide_choice` is word/trigram affinity (`compute_option_affinity`,
  `compute_trigram_similarity`). With `["no","yes"]` every question returns `0.5 / 0.5`; with
  `["safe","destructive"]` it returns `destructive 1.0` for `ls -la` as well as `rm -rf /`. Therefore:
  no ONNX artifact to download, and the backend cannot be validated for quality until a real runtime
  exists. `flat()` in `crates/apc-decision/src/jev.rs` treats the flat answer as no opinion.

## Unverified — remaining
- **`ratatui` + `crossterm`** — v1.1 only, don't research for an MVP task.

## Open questions

- The adapters ignore the `usage` object that OpenAI/Anthropic send in the final SSE chunk; per-token
  cost reporting would need it parsed. Not needed until `apc config` reports spend.
- `open-jev` licence and redistribution rules — affects whether `apc model download` can host the
  artifact or must point at the upstream URL.
- Context compaction strategy: summarize with the same provider (costs a call) vs a local heuristic
  (free but crude). Not decided.
