# Repository Checks

Repository Checks read the whole repository rather than one package's code. They cover dependencies, boundaries, and releases.

<!-- docs-tree:start -->

## Docs

- [Repository Checks](docs/more/developing/testing/repository-checks.md): The checks that read the whole repository rather than one package's code: its layers, its dependencies, its boundaries, its declarations and the rules its tests keep

<!-- docs-tree:end -->

## Code

- [`src/commands.rs`](src/commands.rs): one verb per meaning in every module's commands, against `verbs.toml`
- [`src/legacy.rs`](src/legacy.rs): no code for an older version, against `legacy.toml`
- [`src/lib.rs`](src/lib.rs): the dependency checks, for any workspace's root
- [`src/trust.rs`](src/trust.rs): who publishes each crate, and its refresh
- [`tests/main.rs`](tests/main.rs): the checks, one module each
