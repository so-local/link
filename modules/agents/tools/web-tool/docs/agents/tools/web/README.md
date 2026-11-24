# Web

> Web Pages Your Agent Fetches and Searches

**Your agent reads the web and searches it.** The agent fetches a page as
Markdown and searches with a secret of yours, never one the agent holds.

<!-- quickstart -->

## Add Web to Your Agent

Enable Web for your running agent and review any requested sandbox permissions. Web may need [programs or credentials](guide.md) first.

```sh
lnk agent start
lnk agent tool add web
```

## List Your Agent's Tools

Check which tools your agent has enabled, and confirm that Web is on the list.

```sh
lnk agent tool list
```

## Ask Your Agent to Use Web

Open a terminal conversation and ask your agent to use Web. Learn which [actions Web supports and where its access ends](guide.md).

```sh
lnk agent chat
```

A harness starts `lnk-web-tool` itself and gives the tool the command line that its [declarations](more/command-line.md) define. You never type the tool's commands. The answers to a fetch and a search come from the tool's [contract crate](../../../../contract/docs/agents/tools/web/more/contract.md), which other modules name.

## Learn How Web Works

Read why Web is built as it is in [Decisions](more/decisions.md), how the tool's process and calls fit together in [Developing](more/developing.md), and what to do when the tool fails in [Troubleshooting](troubleshooting.md).

<!-- docs-tree:start -->

## Docs

### Pages

- [Web Guide](guide.md): Enable Web to fetch pages as text and optionally search with a Brave Search or Tavily account

### More

- [Developing](more/developing.md): `lnk-web-tool` is Link Web: an MCP server over stdio with two calls, `fetch` and `search`, for any harness to reach as a tool
- [Web Tool Command Line](more/command-line.md): `lnk-web-tool`'s command line is declared in the module's `actions.toml`, and its parser is generated from that file when the tool builds, so the commands a harness runs and the ones the tool reads can't differ
- [Decisions](more/decisions.md): How your agent reaches the web, and whose search it uses
- [Troubleshooting](troubleshooting.md): What to do when your agent's web tool doesn't search or fetch a page

<!-- docs-tree:end -->
