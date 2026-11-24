# Browser

> Browser Pages Your Agent Clicks and Fills In

**Your agent uses a browser.** The agent opens pages, reads them, clicks,
types into forms, and sees screenshots, even on pages that need scripts,
a sign-in, or a button press, which a
[Web](../../../../../web-tool/docs/agents/tools/web/README.md) fetch can't do.
Browser works on Linux only today.

<!-- quickstart -->

## Add Browser to Your Agent

Enable Browser for your running agent and review any requested sandbox permissions. Browser may need [programs or credentials](guide.md) first.

```sh
lnk agent start
lnk agent tool add browser
```

## List Your Agent's Tools

Check which tools your agent has enabled, and confirm that Browser is on the list.

```sh
lnk agent tool list
```

## Ask Your Agent to Use Browser

Open a terminal conversation and ask your agent to use Browser. Learn which [actions Browser supports and where its access ends](guide.md).

```sh
lnk agent chat
```

Local Link starts `lnk-browser-tool` itself and [builds its command line](../../../../contract/docs/agents/tools/browser/more/command-line.md) from the tool's own types. You never type the tool's commands.

## Learn How Browser Works

Read why Browser is built as it is in [Decisions](more/decisions.md), how the tool's process and calls fit together in [Developing](more/developing.md), and what to do when the tool fails in [Troubleshooting](troubleshooting.md).

<!-- docs-tree:start -->

## Docs

### Pages

- [Browser Guide](guide.md): Enable Browser to open pages, fill forms, and inspect screenshots with a browser profile reserved for your agent

### More

- [Developing](more/developing.md): `lnk-browser-tool` is Link Browser: an MCP server over stdio whose calls drive a headless Chromium, for any harness to reach as a tool
- [Decisions](more/decisions.md): Why Link Browser drives a browser of its own, in a sandbox, and leaves the network rule to the sandbox's proxy
- [Troubleshooting](troubleshooting.md): What to do when your agent's browser doesn't start, reach a page or keep a sign-in

<!-- docs-tree:end -->
