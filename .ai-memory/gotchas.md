# Gotchas

## Repository reality vs README

- The README is the **target `v1.0.0` design**. The tree is docs-only: no `Cargo.toml`, no `crates/`,
  no CI, no tests, no `skills/`, no `CONTRIBUTING.md`, no `.github/ISSUE_TEMPLATE/` (the README links
  to files that do not exist). Never run `cargo build` "to check" without looking at the tree first.
- Any task naming an `apc-*` crate means *create* it, not edit it.
- Commit history is README edits only (`510f1a9`, `38e0c20`, `589921c`, `5298e17`) — there is no
  code history to learn from.

## Rust edition / MSRV

- README claims Rust **1.80+**. Edition 2024 requires **1.85+**, so use **edition 2021** and
  `resolver = "2"` unless the MSRV is deliberately raised. Using edition 2024 silently bumps the
  toolchain requirement for users.
- `rust-version = "1.80"` in `[workspace.package]` is what keeps `cargo install` honest.

## Async traits

- Native `async fn` in traits (stable since 1.75) is **not dyn-compatible**. Since "everything is a
  plugin" means `Box<dyn ModelAdapter>` / `Arc<dyn Tool>`, the `#[async_trait]` attribute from
  `dtolnay/async-trait` is required — that is why the README shows it.
- Without a runtime, `async fn` in a `Tool` impl still needs `Send + Sync` bounds so tools can live
  behind `Arc` across tasks.

## Binary size budget (< 15 MB)

- This is the easiest constraint to break. Avoid pulling `openssl-sys` (prefer `rustls`), avoid a
  bundled tokenizer/runtime, and keep `strip = true` + `lto = true` in the release profile.
- Any new dependency that compiles a with-rust codebase (e.g. a full ML runtime) blows the budget —
  the decision model must be downloaded at runtime, not linked.

## Model downloads

- ~350 MB for `open-jev`; the download path must be resumable and SHA-256 verified. A user hitting
  Ctrl-C mid-download must not leave a half-written model file that later loads silently.

## Task tracking (kaneo MCP)

- Use workspace `GAiYlKA3Nr2yEh3Ea40IY0f0tvK4rjQ0` and project `f0yyfbfaoe4610r2d2hc8qir`.
  Don't create a parallel workspace/project.

## Things that are easy to get wrong in naming

- `aplix-code` = package/repo, `apc` = binary. `cargo install aplix-code` produces `apc`.
- Config file is `apc.toml` (not `aplix.toml`, not `.apcrc`).
