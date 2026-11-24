# Run

> Run Shell Commands, Each in Its Own Sandbox

**Your agent runs shell commands in the agent's files folder, each in a
sandbox of its own.** Add Run when you want your agent to have the
command line, most often to [run Python or JavaScript](scripts.md):
editing many files at once, working out sums, or querying a table.

<!-- quickstart -->

## Add Run to Your Agent

Start your agent and enable the run tool, then review any requested permissions. Check what Run [requires and what the sandbox grants](guide.md).

```sh
lnk agent start
lnk agent tool add run
```

## Show What Your Agent May Run

Inspect the instructions your agent receives, including the programs the agent may run. You can [choose what the agent may run](guide.md#choose-what-it-may-run) with sandbox command grants.

```sh
lnk agent show --instructions
```

## Ask Your Agent to Run a Program

Ask your running agent to run a program in the agent's files folder. Learn [what Run can do and how to configure access](guide.md).

```sh
lnk agent chat
```

Local Link starts `lnk-run-tool` itself and [builds its command line](../../../../contract/docs/agents/tools/run/more/command-line.md) from the tool's own types. You never type the tool's commands.

## Learn How Run Works

Read why Run is built as it is in [Decisions](more/decisions.md), how the tool's process and calls fit together in [Developing](more/developing.md), and what to do when the tool fails in [Troubleshooting](troubleshooting.md).

<!-- docs-tree:start -->

## Docs

### Pages

- [Run Guide](guide.md): Enable Run to execute shell commands in your agent’s files folder
- [Run Scripts](scripts.md): Give your agent Python or JavaScript, the two languages models write best, and it edits many files at once, works out sums and statistics, and queries tables, in a sandbox that reaches only what you grant

### More

- [Developing](more/developing.md): `lnk-run-tool` is Link Run: an MCP server over stdio that runs an agent's shell commands, each call in a sandbox of its own
- [Decisions](more/decisions.md): Why each `run` call gets a sandbox of its own, which programs it offers, and how a command runs in the background
- [Troubleshooting](troubleshooting.md): What to do when your agent can't run a program, reach a host, or keep a command running

<!-- docs-tree:end -->
