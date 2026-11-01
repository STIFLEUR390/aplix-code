# Commands

**None of these work today** — there is no `Cargo.toml` yet. They are the target commands; verify
against the real manifest once the workspace exists.

## Build / run

```bash
cargo build --release
./target/release/apc --version
cargo run -p apc-cli -- run "explain src/main.rs"
```

Requires Rust 1.80+. Only system dependency expected: OpenSSL, or `rustls` if the feature is enabled.

## Verify (run in this order before claiming done)

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

## Single test / single crate

```bash
cargo test -p apc-decision                 # one crate
cargo test -p apc-decision noul            # one test (substring match)
cargo test permission_guard -- --nocapture # one test + stdout
```

## Workspace scaffolding (what v0.1 needs)

```bash
cargo new --lib crates/apc-core      # repeat per crate; apc-cli is a bin
cargo add --git ... # nothing to add yet
```

Root manifest needs `[workspace] members = ["crates/*"]`, `resolver = "2"`, `[workspace.package]`
and `[workspace.dependencies]` (see `research.md`).

## Model management (target UX)

```bash
apc init                        # writes apc.toml
apc model list
apc model download open-jev     # ~350 MB, resumable + SHA-256 verified
apc model status
apc model verify
apc model remove open-jev
```

## Docs lookup (this machine)

Context7 CLI, no global install needed:

```bash
npx ctx7@latest library "Rust" "Cargo workspace dependency inheritance"
npx ctx7@latest docs /websites/rs_genai "tool calling in chat completions"
```

Verified IDs already found: `/websites/rs_genai` (genai, latest 0.5.2), `/jeremychone/rust-genai`
(v0.4.0-alpha.7), `/dtolnay/async-trait`, `/rust-lang/rust`, `/rust-lang/reference`.
