# Code Tool Contract

`link-code-tool-contract` holds Link Code's command line, which the agent module builds and `lnk-code-tool` reads.

<!-- docs-tree:start -->

## Docs

- [Code Tool Command Line](docs/agents/tools/code/more/command-line.md): `link-code-tool-contract` gives the callers of `lnk-code-tool` its runtime arguments as types, `CodeToolServeArgs`, and the command line each one's `argv` writes, generated from the tool's `actions.toml`, as the parser the tool reads it…

<!-- docs-tree:end -->

## Code

- [`build.rs`](build.rs): generates the code `cli.rs` includes, from the declarations
- [`cli.rs`](src/cli.rs): its runtime arguments and their command line, generated from the tool's declarations
- [`lib.rs`](src/lib.rs): the crate's root
