# Link MCP

Link MCP offers Local Link commands as tools to a client that speaks the
Model Context Protocol (MCP). The tools include the offline documentation
and the running agent's capabilities.

<!-- docs-tree:start -->

## Docs

### Pages

- [Link MCP](docs/agents/tools/mcp/README.md): Link MCP lets an agent read Local Link's documentation and call the Local Link commands you expose

### References

- [MCP Commands](docs/more/reference/modules/agents/mcp.md): Every command the `mcp` module offers, from its declarations, in order of the resource each acts on

### More

- [Security](docs/agents/tools/mcp/security.md): Command tools retain the calling client's approval boundary and bind agent reads to the selected agent

<!-- docs-tree:end -->

## Code

- [`cli.rs`](src/cli.rs): the server's command line, generated from `actions.toml` and `types.toml`
- [`lib.rs`](src/lib.rs): installed tool discovery, protocol replies and command execution
- [`main.rs`](src/main.rs): command entry point
- [`transport.rs`](src/transport.rs): bounded stdio transport and cancellation
