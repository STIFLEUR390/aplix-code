# Research log

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

## Verified — genai (`/websites/rs_genai`, latest 0.5.2)

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

## Unverified — remaining

- **`local-jev`** — no reliable docs source. Context7's only match (`/qew7/jev-feels`) failed to
  fetch and is probably unrelated; do **not** cite it. Unknown: inference backend, asset size,
  runtime weight. Directly threatens the 15 MB budget → keep it feature-gated.
- **`ratatui` + `crossterm`** — v1.1 only, don't research for an MVP task.

## Open questions

- Does `genai` expose per-token usage/cost per response? Needed if `apc config` ever reports spend.
- `open-jev` licence and redistribution rules — affects whether `apc model download` can host the
  artifact or must point at the upstream URL.
- Context compaction strategy: summarize with the same provider (costs a call) vs a local heuristic
  (free but crude). Not decided.
