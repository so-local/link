# Desktop

> Desktop Apps Your Agent Works with a Mouse

**Your agent works a desktop.** The agent sees the desktop as screenshots
and works it with the mouse and keys, so the agent can use programs with
no command line or API, such as those that
[Run](../../../../../run-tool/docs/agents/tools/run/README.md) starts.

<!-- quickstart -->

## Add Desktop to Your Agent

Enable Desktop for your running agent and review any requested sandbox permissions. Desktop may need [programs or credentials](guide.md) first.

```sh
lnk agent start
lnk agent tool add desktop
```

## List Your Agent's Tools

Check which tools your agent has enabled, and confirm that Desktop is on the list.

```sh
lnk agent tool list
```

## Ask Your Agent to Use Desktop

Open a terminal conversation and ask your agent to use Desktop. Learn which [actions Desktop supports and where its access ends](guide.md).

```sh
lnk agent chat
```

Local Link starts `lnk-desktop-tool` itself and [builds its command line](../../../../contract/docs/agents/tools/desktop/more/command-line.md) from the tool's own types. You never type the tool's commands.

## Learn How Desktop Works

Read why Desktop is built as it is in [Decisions](more/decisions.md), how the tool's process and calls fit together in [Developing](more/developing.md), and what to do when the tool fails in [Troubleshooting](troubleshooting.md).

<!-- docs-tree:start -->

## Docs

### Pages

- [Desktop Guide](guide.md): Enable Desktop on Linux to give your agent a virtual screen, mouse, and keyboard

### More

- [Developing](more/developing.md): `lnk-desktop-tool` is Link Desktop: an MCP server over stdio whose calls see and work an `Xvfb` display, for any harness to reach as a tool
- [Decisions](more/decisions.md): Why Link Desktop gives an agent a virtual desktop of its own
- [Troubleshooting](troubleshooting.md): What to do when your agent's desktop doesn't start, stays black, or a program can't open it

<!-- docs-tree:end -->
