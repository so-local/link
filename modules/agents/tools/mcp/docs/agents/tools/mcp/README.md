# Link MCP

> Local Link's Commands as Tools for Any Agent

Link MCP lets an agent read Local Link's documentation and call the Local
Link commands you expose. A Link agent gets it as a tool, and any other
agent or client that speaks the Model Context Protocol (MCP) can add it
as a server.

<!-- quickstart -->

## Give a Link Agent Link MCP

A Link agent has Link MCP only once you add it. The tool is off by
default, and the Onboarding and Programmer ready-made agents come with it
on. The agent module starts `lnk mcp` for the agent and binds it to that agent, so its
reads show only its own capabilities. Declared reads run without asking,
and every change asks you first through the agent's sandbox.

```sh
lnk agent tool add mcp
```

### Ask Your Agent About Local Link

Ask your agent what it can do or how to enable a capability. Link MCP
can read the offline manual, inspect the agent's status, and list its
enabled tools. Installation alone does not enable a capability, and the
agent cannot grant itself more access.

### Remove the Tool

Removing the tool withholds these command tools. Adding and removing both
save the agent's tool switch, and a stopped agent uses the change at its
next start. A background agent asks before restarting with the change.
Approve that question in the terminal, or use `--yes` with `add` to
approve its restart. Without that approval, the saved change waits for a
restart. An agent running in a terminal keeps its current tools until you
start it again there.

```sh
lnk agent tool remove mcp
```

## Add Local Link to Another MCP Client

MCP is a protocol that lets a program discover named tools and call them
with structured arguments. Claude Code, Codex, Claude Desktop, and other
MCP clients add Local Link as a server when you configure them to start
`lnk mcp` over standard input and output. The server offers the commands
that installed modules declare as tools, including `manual`, agent
context reads, and box lifecycle commands.

```sh
lnk mcp
```

A client that starts `lnk mcp` holds the command authority of the user
running it. Local Link's sandbox approval doesn't apply there, so the
client's own approval policy decides which calls run.

### Inspect Allowed Reads

Print the read-only call names as a JSON array without starting the
server. A client's approval policy can allow these names without asking.
A Link agent's start uses this same list to configure its sandbox.

```sh
lnk mcp --read-calls
```

## Call Commands

Names use two underscores between command words, so [`lnk box list`](../../../../../../../compute/box/docs/services/boxes/guide.md#list-your-boxes) is `box__list`. Inputs follow the owning command's declared argument names and types. The server passes checked arguments directly to `lnk`, without a shell, and closes terminal input. Interactive terminal sessions are not exposed as ordinary calls.

### Read Results

JSON commands return their normal JSON as text and structured content. Lists and scalar values are wrapped in a structured object under `result`. The manual and other text commands return text. A failed command returns a tool error, while an unavailable name or invalid input returns a protocol error before execution.

### Approve Changes

Link agents allow declared reads and ask before each change through their existing sandbox approval path. Another MCP client must provide its own approval policy, as [Add Local Link to Another MCP Client](#add-local-link-to-another-mcp-client) says. [Security](security.md) describes the boundary and its limits.

<!-- docs-tree:start -->

## Docs

- [Security](security.md): Command tools retain the calling client's approval boundary and bind agent reads to the selected agent

<!-- docs-tree:end -->
