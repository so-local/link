---
name: rust
description: Build, test, lint and write idiomatic Rust in a Cargo project, workspaces included.
---

# Rust

## Detect

`Cargo.toml` at the root; `[workspace]` means several crates (`members`).
`rust-toolchain(.toml)` pins the toolchain; `.cargo/config.toml` holds
aliases and flags; `rustfmt.toml` and `clippy.toml` hold style. Check
`cargo --version`. Read CI for the exact flags.

## Commands

```sh
cargo check                       # fastest compile check
cargo build [--release]
cargo test                        # all; add -p crate, or a name filter
cargo test -p crate name -- --nocapture
cargo fmt --all                   # check only: cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo doc --no-deps
cargo tree -d                     # duplicate dependencies
```

In a workspace, work with `-p <crate>` while iterating and run
`--workspace` once at the end. Features: `--all-features` or
`--no-default-features` to check both. Run doc tests via `cargo test`.

## Idioms

- Return `Result<T, E>`; propagate with `?`. Libraries define error enums
  (`thiserror`), binaries may use `anyhow`. No `unwrap`/`expect`/`panic!`
  on input-dependent paths; fine in tests or for proven invariants (say why).
- Borrow (`&str`, `&[T]`, `&Path`) in parameters; own in return values when
  needed. Do not clone to silence the borrow checker; restructure.
- Use iterators, `match` and enums that make illegal states
  unrepresentable; derive `Debug`, `Clone`, `PartialEq` as appropriate.
- Keep `unsafe` out unless required; wrap it in a safe API with a
  `// SAFETY:` comment.
- Concurrency: `Send`/`Sync` bounds, never hold a `std::sync::Mutex` guard
  across `.await`; do not block inside async code (`spawn_blocking`).

## Pitfalls

Changing a public API or `Cargo.toml` features affects dependents. Commit
`Cargo.lock` for applications. `cargo fix`/`clippy --fix` can change more
than you intend: review the diff. Warnings are errors in CI; fix them,
do not `#[allow]` them without a reason. Integration tests in `tests/` see
only the public API. Slow builds: use `cargo check` and `-p`.
