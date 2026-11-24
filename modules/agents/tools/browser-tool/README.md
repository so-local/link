# Link Browser

Link Browser (`lnk-browser-tool`) lets an agent open pages in a Chromium of its own, in a sandbox, then read, click, type into, and screenshot them.

<!-- docs-tree:start -->

## Docs

### Pages

- [Browser](docs/agents/tools/browser/README.md): Your agent uses a browser
  - [Browser Guide](docs/agents/tools/browser/guide.md): Enable Browser to open pages, fill forms, and inspect screenshots with a browser profile reserved for your agent

### Packages

- [Browser Tool Contract](contract/README.md): `link-browser-tool-contract` holds Link Browser's command line, which the agent module builds and `lnk-browser-tool` reads

### More

- [Developing](docs/agents/tools/browser/more/developing.md): `lnk-browser-tool` is Link Browser: an MCP server over stdio whose calls drive a headless Chromium, for any harness to reach as a tool
- [Decisions](docs/agents/tools/browser/more/decisions.md): Why Link Browser drives a browser of its own, in a sandbox, and leaves the network rule to the sandbox's proxy
- [Troubleshooting](docs/agents/tools/browser/troubleshooting.md): What to do when your agent's browser doesn't start, reach a page or keep a sign-in

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): the browser and its pages
- [`main.rs`](src/main.rs): the program
- [`mcp.rs`](src/mcp.rs): the MCP server and its tools
