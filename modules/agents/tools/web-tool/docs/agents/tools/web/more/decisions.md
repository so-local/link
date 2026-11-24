# Decisions

How your agent reaches the web, and whose search it uses.

## How does your agent reach the web?

Through Link Web, a tool like any other, added with [`lnk sandbox tool add web`](../../../../../../../../security/sandbox/docs/security/sandbox/reach.md#call-a-tool-that-runs-outside), then allowed with [`lnk agent permission add --tools.names web`](../../../../../../../agent/docs/security/sandbox/permissions/README.md#choose-what-your-agent-can-reach), so it works in every harness and its
calls go through the agent's proxy, logged and gated. It runs outside
the agent's sandbox, so a search API's key is in the tool's own
environment and never the harness's. A tool rather than code in a
harness keeps the harness small, and lets you swap it for another search
tool by its name.

## Why is a page on your network never fetched?

The tool runs outside the sandbox, so a fetch could reach what the
sandbox keeps out: this computer's servers, your network, a cloud's
metadata service. It keeps the same rule as the sandbox's proxy, from
the same code: every address a name resolves to is checked, the
connection is made only to those addresses, and each redirect is checked
again. A host you trust is named with `--private-host`, and `fetch`'s
description names each such host, or none, to the model: told only that
such pages aren't fetched, a model refuses a page on a host you named
without calling the tool.

## Why does a fetch give markdown, a part at a time?

A model reads markdown as well as HTML at a fraction of the tokens, and
the tags around the content (scripts, styles, menus) are most of a page.
A part's size is an opinion, `[tool.web] fetch` in the specification, and 24 KB
in Link's, about 6,000 tokens, under what Link Harness keeps of a tool's
answer. Each part says where the next starts, as memory’s calls do. Keeping a recent page for [its later parts](../guide.md#a-long-page) avoids repeated downloads and lets those parts describe the same copy.

## How does the tool learn the agent's opinions?

Link Harness sends each call the called tool's own table of the agent's
specification, in its `_meta` (`link.local/spec`). One tool serves every agent,
so its opinions can't be set when it's added, and a call is the only
place each agent's reaches it. A tool that reads none ignores it. Link
Web refuses a fetch that comes without it: one agent's `hosts` would
otherwise be lost through any harness that sends none, so it fails
closed, and only search works there.

## Which search APIs does it use?

Brave Search's and Tavily's: both answer a query with titles, addresses
and snippets over plain HTTPS with one key, and both can keep to the
last day, week, month or year, and either gives a key to anyone who
signs up. `LNK_WEB_SEARCH_URL` points it at another server that answers
as one of them does.
