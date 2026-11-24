# Tools Guide

Choose which tool providers your agent can use, and check their harness
requirements and action namespaces. List the tools currently enabled
before adding another.

```sh
lnk agent tool list
```

## Link's Tools

Link has eight tool providers. Their managed actions use the namespaces
below, with availability depending on the harness and enabled grants.

| Tool | What it does |
|---|---|
| [Memory](../../../../memory-tool/docs/agents/tools/memory/README.md) | Its files and conversations, searched by meaning |
| [Scheduler](../../../../scheduler-tool/docs/agents/tools/scheduler/README.md) | Subagents, schedules and messages from others |
| [Web](../../../../web-tool/docs/agents/tools/web/README.md) | A page as markdown, and a search with your key |
| [Browser](../../../../browser-tool/docs/agents/tools/browser/README.md) | A real browser, sandboxed, read and clicked, on Linux |
| [Desktop](../../../../desktop-tool/docs/agents/tools/desktop/README.md) | A virtual desktop, its mouse, keys and screenshots |
| [Run](../../../../run-tool/docs/agents/tools/run/README.md) | Programs, each call sandboxed, long ones behind |
| [File](../../../../file-tool/docs/agents/tools/file/README.md) | Its files read, written and changed |
| [Code](../../../../code-tool/docs/agents/tools/code/README.md) | What language servers find wrong, and names |

Each tool runs as an MCP server outside your agent's sandbox, and your
agent calls it through its proxy. Each tool's page says what it does
in full: words and meaning in Memory's search, the browser's
sandbox, the desktop's mouse and keys, Run's background programs.

### Give your agent one

[`lnk agent tool list`](../../../../../agent/docs/agents/README.md#add-tools-and-skills) shows which your agent has, and `lnk agent tool
add memory`, `add web`, `add browser` or `add desktop` gives it one;
its first [`lnk agent start`](../../../../../agent/docs/agents/README.md#create-an-agent) offers them.

### Which harness has which

Memory, Web, Browser and Desktop work in any harness, Scheduler in any
harness that reads who a message is from (every harness that comes with
Link does), and Run, File and Code in any harness that keeps to the
files folder's layout. Today those are Link Harness, since the other
harnesses bring files, a shell and code of their own. Browser and Desktop
run on Linux only.

### Off until you add them

Scheduler, Run, File and Code come with Link Harness, each off until
[`lnk agent tool add <name>`](../../../../../agent/docs/agents/README.md#add-tools-and-skills) turns it on, or its agent's specification says
`[tool.<name>] enabled = true`. A tool knows
nothing of a harness: what it reads of an agent, it reads in a
documented format.

## A harness's own tools

A harness may bring tools of its own, such as a shell and a file editor,
and Link's tools sit beside them. Link turns off a harness's own
scheduler so no job fires twice (Hermes and OpenClaw). Link adds its tools in the harness's own
MCP settings and removes only its own. Each harness's page
says what it keeps: [Harnesses](../../../../../agent/docs/agents/harnesses/use/README.md).

## Namespaces

Every harness receives Link's actions by namespace. `task` combines run
and scheduler: `run`, `start`, `read`, `agent`, `list` and `stop`, with
`send` for messages to retained children. `cron` holds the scheduler's
schedules: `create`, `delete` and `list`. `file` combines file
operations and code lookups. `agent` offers `send`; web, browser,
desktop and memory keep their namespace names.

### Task kinds

`task__list` lists runs and schedules together; `kind` selects `run` or
`schedule`. When both providers are present, `task__stop` requires that
selector so an ambiguous identifier never stops the wrong kind of work.

### Provider policies

Each call uses its provider's existing confinement and current approval
policy. The namespace does not grant more access. Changing or removing a
provider takes effect on the next call, and a changed launch requires a
new namespace session.

## Test the tools

The tools are members of the workspace, so a test builds them beside `lnk`, and some run the core's `lnk` from the workspace's `target/debug`. [Tests that run inside the sandbox skip without bubblewrap](../../../../../../../packages/test-kit/docs/more/developing/testing/test-environments.md) unless `LNK_REQUIRE_SANDBOX=1` makes the missing program a failure, as CI sets it.
