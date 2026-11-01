# Research log (verified via Context7 / docs.rs)

Sources fetched with `npx ctx7@latest`. Everything below was read from current published docs, not
recalled. Re-verify with a `docs` call if it starts to matter (versions move).

## Cargo workspaces

- Root manifest: `[workspace] members = [...]`, `resolver = "2"` for edition-2021 workspaces
  (resolver "3" for edition 2024 / MSRV 1.84+). Rust's own repo lists members alphabetically —
  cheap convention to copy.
- `[workspace.package]` centralises `version`, `edition`, `rust-version`, `license`, `authors`;
  members inherit with `version.workspace = true` etc. Same for `[workspace.dependencies]` +
  `serde = { workspace = true }` — this is the mechanism that keeps nine crates on one dependency
  version.
- A workspace has **one `Cargo.lock` at its root**, shared by all members
  (`default_lock_root` → `self.root()` in cargo's `workspace.rs`). Don't create per-crate lockfiles.
- Edition-2024 members may override an inherited dependency's `default-features`; on edition 2021
  `default-features = false` on an inherited dep is **ignored with a warning** (RFC 3945, Cargo
  1.99 changelog). Practical consequence for this repo on edition 2021: declare feature choices on
  the `[workspace.dependencies]` entry, not per member.

## async-trait

- `/dtolnay/async-trait`: exists precisely because native async-fn-in-trait is not supported for
  `dyn Trait`. Required here because the plugin architecture stores trait objects.
- Cost: each async method is boxed into a `Pin<Box<dyn Future>>`. Fine for the agent loop, worth
  remembering if a hot path ever lives behind a trait object.

## genai (LLM adapters) — `/websites/rs_genai`, latest 0.5.2

- Client construction: `Client::builder()` → `ClientBuilder` with `with_reqwest`,
  `with_config(ClientConfig)`, `with_chat_options(ChatOptions)`, `with_auth_resolver`,
  `with_service_target_resolver`, `with_model_mapper`, `with_adapter_kind(AdapterKind)`, then
  `build()`.
- `with_adapter_kind` binds a client to one provider and routes directly instead of inferring the
  adapter from the model name on every call — the right shape for one `ModelAdapter` per provider.
- Requests: `ChatRequest::from_user("weather")`, `.with_tools(vec![Tool::new("get_weather")])`.
  Options: `ChatOptions::default().with_tool_choice(ToolChoice::tool("get_weather"))`;
  `Client::exec_chat` executes.
- Tool plumbing: the model replies with `tool_calls`; the response is fed back as a message with
  role `tool`, `content` = result, `tool_call_id` = the call's `call_id`. genai models this as
  `ContentPart::ToolResponse { content, call_id }` inside `ChatRole::Tool`.
- Strict tool mode: OpenAI rejects object schemas without `additionalProperties: false`; genai adds
  it automatically when `tool.strict == true`. Also give every tool a JSON-schema `parameters` value.
- DeepSeek/Kimi require `reasoning_content` echoed back in the assistant message.
- Auth/target primitives: `AuthData::from_single("key")`, `ServiceTarget { model, auth, endpoint }`
  — useful when `apc-models` needs a custom endpoint (Ollama, self-hosted gateway).

## Not yet researched (do when the task lands)

- `clap` derive layout for the `apc` subcommand tree.
- Resumable HTTP download with SHA-256 (reqwest `Range` + streaming digest) — no crate chosen yet.
- `candle`/`ort` inference for `open-jev` (DeBERTa-v3) — the binary-size budget makes this the
  riskiest dependency choice in the project.
