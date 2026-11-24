# Tool Calls

A [tool](../README.md) is an MCP (Model Context Protocol) server that runs
outside the sandbox, and each thing it can do is a call. Every call asks
you first unless you allow it by name, so you need its server's own name
for each call before you allow it.

## List a Tool's Calls

`lnk sandbox tool-call list` starts the tool's server here and asks it
for its calls. It prints each one with whether it asks or is allowed,
what the server says of it (that it only reads, or that it may delete),
and its description. What a server says of its calls is its own word,
and never skips asking.

```sh
lnk sandbox tool-call list gh
lnk sandbox tool add gh --add list_issues --add get_issue -- github-mcp-server stdio
```

[Adding a tool again](../../reach.md#call-a-tool-that-runs-outside) with `--add` replaces its allowed calls.
