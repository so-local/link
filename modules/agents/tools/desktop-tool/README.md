# Link Desktop

Link Desktop (`lnk-desktop-tool`) gives an agent a virtual desktop of its own, which the agent sees as screenshots and works with the mouse and keys.

<!-- docs-tree:start -->

## Docs

### Pages

- [Desktop](docs/agents/tools/desktop/README.md): Your agent works a desktop
  - [Desktop Guide](docs/agents/tools/desktop/guide.md): Enable Desktop on Linux to give your agent a virtual screen, mouse, and keyboard

### Packages

- [Desktop Tool Contract](contract/README.md): `link-desktop-tool-contract` holds Link Desktop's command line, which the agent module builds and `lnk-desktop-tool` reads

### More

- [Developing](docs/agents/tools/desktop/more/developing.md): `lnk-desktop-tool` is Link Desktop: an MCP server over stdio whose calls see and work an `Xvfb` display, for any harness to reach as a tool
- [Decisions](docs/agents/tools/desktop/more/decisions.md): Why Link Desktop gives an agent a virtual desktop of its own
- [Troubleshooting](docs/agents/tools/desktop/troubleshooting.md): What to do when your agent's desktop doesn't start, stays black, or a program can't open it

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): the desktop, on `Xvfb`
- [`main.rs`](src/main.rs): the program
- [`mcp.rs`](src/mcp.rs): the MCP server and its tools
