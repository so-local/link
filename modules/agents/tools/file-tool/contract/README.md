# File Tool Contract

`link-file-tool-contract` holds Link File's command line, which the agent module and Local Link's units build and `lnk-file-tool` reads.

<!-- docs-tree:start -->

## Docs

- [File Tool Command Line](docs/agents/tools/file/more/command-line.md): `link-file-tool-contract` gives the callers of Link file, `lnk-file-tool`, each command's runtime arguments as a Rust type and the command line that gives them

<!-- docs-tree:end -->

## Code

- [`build.rs`](build.rs): generates the code `cli.rs` includes, from the declarations
- [`checks.rs`](src/checks.rs): the check hooks the declarations name
- [`cli.rs`](src/cli.rs): its runtime arguments and their command line, generated from the tool's declarations
- [`lib.rs`](src/lib.rs): the crate's root
