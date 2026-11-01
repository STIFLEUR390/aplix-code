# Commands

**None of these work today** — there is no `Cargo.toml` yet. They are the target commands; verify
against the real manifest once the workspace exists.

## Build / run

```bash
cargo build --release
./target/release/apc --version        # also prints the loaded decision model
cargo run -p apc-cli -- run "explain src/main.rs"
cargo run -p apc-cli -- chat --continue
```

Requires Rust 1.80+. Only system dependency expected: OpenSSL, or `rustls` if the feature is enabled.

## Verify (in this order before claiming done)

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
ls -lh target/release/apc             # must stay under 15 MB
```

## Single test / single crate

```bash
cargo test -p apc-decision                 # one crate
cargo test -p apc-decision permission      # one test (substring match)
cargo test permission_guard -- --nocapture # one test + stdout
```

## Workspace scaffolding (v0.1)

```bash
cargo new --lib crates/apc-core      # repeat per crate; apc-cli is a bin
```

Root manifest: `[workspace] members = ["crates/*"]`, `resolver = "2"`, `[workspace.package]`,
`[workspace.dependencies]` (see `research.md`), plus the release profile:

```toml
[profile.release]
lto = true
codegen-units = 1
strip = "symbols"
panic = "abort"
opt-level = 3
```

## CLI surface (target)

```bash
apc init                        # writes a commented apc.toml
apc config                      # effective config + which layer set each key (P1)
apc run "<prompt>"
apc chat [--continue]
apc model list
apc model download open-jev     # ~350 MB, resumable + SHA-256 verified
apc model status
apc model verify
apc model remove open-jev
```

Model cache and sessions live under `${XDG_DATA_HOME:-~/.local/share}/aplix/`.

## Docs lookup (this machine)

Context7 CLI, no global install needed:

```bash
npx ctx7@latest library "Rust" "Cargo workspace dependency inheritance"
npx ctx7@latest docs /websites/rs_genai "tool calling in chat completions"
```

Verified IDs already found: `/websites/rs_genai` (genai, latest 0.5.2), `/jeremychone/rust-genai`
(v0.4.0-alpha.7), `/dtolnay/async-trait`, `/rust-lang/rust`, `/rust-lang/reference`.
