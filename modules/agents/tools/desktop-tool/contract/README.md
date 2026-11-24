# Desktop Tool Contract

`link-desktop-tool-contract` holds Link Desktop's command line, which the agent module builds and `lnk-desktop-tool` reads.

<!-- docs-tree:start -->

## Docs

- [Desktop Tool Command Line](docs/agents/tools/desktop/more/command-line.md): `link-desktop-tool-contract` gives the callers of `lnk-desktop-tool` its runtime arguments as types, `CookieCreateArgs` and `DesktopToolServeArgs`, and the command line each one's `argv` writes, generated from the tool's `actions.toml`,…

<!-- docs-tree:end -->

## Code

- [`build.rs`](build.rs): generates the code `cli.rs` includes, from the declarations
- [`cli.rs`](src/cli.rs): its runtime arguments and their command line, generated from the tool's declarations
- [`lib.rs`](src/lib.rs): the crate's root
