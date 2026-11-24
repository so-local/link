# Link Web

Link Web (`lnk-web-tool`) lets an agent fetch a page as Markdown and search the web through a search API with a secret of yours.

<!-- docs-tree:start -->

## Docs

### Pages

- [Web](docs/agents/tools/web/README.md): Your agent reads the web and searches it
  - [Web Guide](docs/agents/tools/web/guide.md): Enable Web to fetch pages as text and optionally search with a Brave Search or Tavily account

### Packages

- [Web Contract](contract/README.md): `link-web-tool-contract` holds what Link Web answers, a page and search results, as shapes that the agent module and other callers name

### More

- [Developing](docs/agents/tools/web/more/developing.md): `lnk-web-tool` is Link Web: an MCP server over stdio with two calls, `fetch` and `search`, for any harness to reach as a tool
- [Web Tool Command Line](docs/agents/tools/web/more/command-line.md): `lnk-web-tool`'s command line is declared in the module's `actions.toml`, and its parser is generated from that file when the tool builds, so the commands a harness runs and the ones the tool reads can't differ
- [Decisions](docs/agents/tools/web/more/decisions.md): How your agent reaches the web, and whose search it uses
- [Troubleshooting](docs/agents/tools/web/troubleshooting.md): What to do when your agent's web tool doesn't search or fetch a page

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): fetch and search
- [`main.rs`](src/main.rs): the program
- [`mcp.rs`](src/mcp.rs): the MCP server
