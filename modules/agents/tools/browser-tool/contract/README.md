# Browser Tool Contract

`link-browser-tool-contract` holds Link Browser's command line, which the agent module builds and `lnk-browser-tool` reads.

<!-- docs-tree:start -->

## Docs

- [Browser Tool Command Line](docs/agents/tools/browser/more/command-line.md): `link-browser-tool-contract` gives the callers of `lnk-browser-tool` its runtime arguments as types, `BrowserToolServeArgs`, and the command line each one's `argv` writes, generated from the tool's `actions.toml`, as the parser the tool…

<!-- docs-tree:end -->

## Code

- [`build.rs`](build.rs): generates the code `cli.rs` includes, from the declarations
- [`cli.rs`](src/cli.rs): its runtime arguments and their command line, generated from the tool's declarations
- [`constants.rs`](src/constants.rs): what marks a file as a script
- [`lib.rs`](src/lib.rs): the crate's root
- [`program.rs`](src/program.rs): the Chromium program an agent's specification names, checked: an executable program by its absolute path, never a script
