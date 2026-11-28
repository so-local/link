# Tools and Skills

> Tools and Skills for Your Agent

Choose the tools your agent can call, and add reusable instructions for recurring work.

```sh
lnk agent tool list
```

## Give It Tools

Add Local Link's tools to your agent, or take them away, one at a time.

```sh
lnk agent tool add web
lnk agent tool remove web
```

`State` is `on` when the tool is configured for this agent, and `off`
otherwise. An installed module alone does not enable its tool. File starts
on, so when you ask, your agent changes its files, instructions, and skills.
Scheduler, run, and code start off in every profile.

### Set Switches and Grants

`add` and `remove` save
the tool's `[behavior.tool.<name>] enabled` switch in `~/Link/Safe/Agents/<agent>/agent.toml`, preserving
other opinions and comments. You can also set that switch yourself.
Grant the folders or programs each tool needs in its sandbox file.
Only confirming a proposal widens those grants. The tool refuses a request outside them.
Saved memory uses its separate notes permission.

### Choose a Capability

Choose the capability you need. Each adds an independent
tool to the same agent, and installing every tool is optional.

```sh
lnk agent tool add mcp|memory|browser|desktop|file|scheduler
```

### Run Local Link Commands

[Link MCP](../../../../../tools/mcp/docs/agents/tools/mcp/README.md) reads documentation, inspects capabilities, and offers explicitly declared commands. Read calls are allowed, while changes ask through the sandbox. Link MCP is off by default, and the Onboarding ready-made agent comes with it on. Add it with `lnk agent tool add mcp`.

### Approve Missing Sandbox Permissions

Adding a built-in tool displays any missing file or program permissions
and asks before saving them in `~/Link/Safe/Agents/<agent>/sandbox.toml`. Without approval,
the tool stays off. `--yes` explicitly approves the displayed proposal.
Existing sufficient grants are reused. Removing a tool switches it off
without revoking grants that another tool may share. Setup and tool
switches live together in `~/Link/Safe/Agents/<agent>/agent.toml` under `[behavior]`.

### Add Web

`add web` installs Link Web if it isn't here, and takes its search key
from `BRAVE_API_KEY` or `TAVILY_API_KEY`, else asks for it. Without
a key, your agent fetches pages but doesn't search. The key stays with
Local Link's sandbox, outside your agent. [`lnk sandbox tool list`](../../../../../../security/sandbox/docs/tooling/sandbox/tools/README.md#tools-and-asking) shows Web's shared
definition, and [`lnk sandbox tool remove web`](../../../../../../security/sandbox/docs/tooling/sandbox/reach.md#call-a-tool-that-runs-outside) removes it for every agent.

### Add Web on a New Machine

An agent's sandbox file can name web on a machine where it was never
added, such as one you imported the agent to. The agent then starts
without web and says so, with `lnk agent tool add web --agent <agent>`
as the step that adds web here. A tool of your own that was removed
since says instead how to forget it.

### Add a Tool of Your Own

Add a tool of your own, an MCP server
[`lnk sandbox tool add`](../../../../../../security/sandbox/docs/tooling/sandbox/reach.md#call-a-tool-that-runs-outside) defines, by its name. Each change says
where it's written. [Local Link's tools](../../../../../tools/docs/agents/tools/README.md) has each one.

### Restart for a Change

A running agent gets a change when it restarts, and each change asks
before restarting the agent. `lnk agent tool add --yes` restarts the agent without
asking. With no terminal to ask in, the agent keeps running as it was, and
`lnk agent restart` gives it the change.

## Inspect Available Actions

In Link Harness, `/tool:list` reports the namespaces and action names
exposed to the current agent. The command reads capability metadata without
enabling a provider or granting a tool. [Command rules](../../../../../harnesses/link/docs/agents/harnesses/link/commands/README.md)
explain owner-only slash invocation and unavailable commands.

## Add Reusable Skills

Add task instructions your agent can read and reuse with [Skills](skill/README.md). Skills are ordinary files you can inspect and edit, and installing one grants no extra permissions.

## Understand Tool Boundaries

What the memory, network, browser, program, and file tools reach is in [Security](security.md). Scheduled work, bounded delegation, and retained delivery are in [Decisions](decisions.md).

## Contribute Here

A new tool gives agents something they can't do today, on every harness
that runs them. A tool is a module, and
[Extend Link with a Module](../../../../../../../docs/more/developing/modules.md)
shows how one is declared, built, and tested.

<!-- docs-tree:start -->

## Docs

### Pages

- [Skills](skill/README.md): Skills give your agent reusable instructions for a kind of task

### More

- [Decisions](decisions.md): Why scheduled work and subagents use bounded tools and retained message delivery
- [Security](security.md): What Memory, Web, Browser, Run, File, and Desktop can reach, and how their work stays within the agent's grants

<!-- docs-tree:end -->
