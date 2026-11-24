# Run Tool Contract

`link-run-tool-contract` holds Link Run's command line, which the agent module builds and `lnk-run-tool` reads.

<!-- docs-tree:start -->

## Docs

- [Run Tool Command Line](docs/agents/tools/run/more/command-line.md): `link-run-tool-contract` gives the callers of Link run, `lnk-run-tool`, each command's runtime arguments as a Rust type and the command line that gives them

<!-- docs-tree:end -->

## Code

- [`build.rs`](build.rs): generates the code `cli.rs` includes, from the declarations
- [`checks.rs`](src/checks.rs): the check hooks the declarations name
- [`cli.rs`](src/cli.rs): its runtime arguments and their command line, generated from the tool's declarations
- [`lib.rs`](src/lib.rs): the crate's root
