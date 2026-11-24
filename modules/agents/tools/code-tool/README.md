# Link Code

Link Code (`lnk-code-tool`) lets an agent ask a project's language servers for diagnostics, definitions, and references, each server in a sandbox.

<!-- docs-tree:start -->

## Docs

### Pages

- [Code](docs/agents/tools/code/README.md): Your agent asks the project's language servers what is wrong in a file, where a name is defined and where it's used, instead of searching the text
  - [Code Guide](docs/agents/tools/code/guide.md): Enable Code to ask installed language servers for diagnostics, definitions, and references

### Packages

- [Code Tool Contract](contract/README.md): `link-code-tool-contract` holds Link Code's command line, which the agent module builds and `lnk-code-tool` reads

### More

- [Developing](docs/agents/tools/code/more/developing.md): `lnk-code-tool` is Link Code: an MCP server over stdio that asks a project's language servers for diagnostics, definitions and references, each server in a sandbox
- [Decisions](docs/agents/tools/code/more/decisions.md): Why `code` asks language servers, and why each runs in a sandbox with no network
- [Troubleshooting](docs/agents/tools/code/troubleshooting.md): What to do when [your agent has no `code`](README.md), or a language server doesn't start or answer

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): the MCP server and its tools
- [`lsp.rs`](src/lsp.rs): a language server's client
- [`main.rs`](src/main.rs): the program
