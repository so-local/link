# Developing

`lnk-web-tool` is Link Web: an MCP server over stdio with two calls,
`fetch` and `search`, for any harness to reach as a tool. How to use
it, and why it works the way it does: [Link Web](../README.md).

## What it reads of a harness

It reads one thing of a harness's. A call's `_meta` may hold
`"link.local/spec"`, the agent's specification's table for the tool: its
`fetch` sets the part's size and its `hosts` the hosts it may fetch.
Link Harness sends it, an empty table where the agent sets neither, and
a fetch that comes without it is refused (`asked` in `src/mcp.rs`).

## Build and test

From the repository root, run `cargo build -p link-web-tool`, then `cargo test -p link-web-tool`.

### Its tests

`tests/mcp.rs` drives it over stdio against pages and search APIs
served on this machine, named with `--private-host 127.0.0.1`, and
reaches the search API at `LNK_WEB_SEARCH_URL`. Only a debug build, the
one tests run, reads that variable, and it takes an `https://` address,
or `http://` only to this machine's loopback, so the key never crosses
a network unencrypted. A release-profile run skips the searches through
the test's gate, as its build would send the key to the real APIs. No
test reaches the internet or spends a search.

### Its limits

`src/limits.toml` holds what the tool admits from its harness, the
longest message line it reads, and how much of a page it reads to
decide how to read it: the start a `<meta>` charset is looked for in,
the start a body of no type is sniffed by, the size of each read, and
the longest character reference. They're read once into
`limits::Limits`.

### The pinned-address fixture

The pinned-address unit fixture runs in a child without inherited proxy
variables, so its synthetic hostname is resolved by the client's pinned
resolver. Other tests keep their environment; the fixture never changes
process-wide variables while tests run concurrently.

## Direct Tool Setup

Manual Web registration supplies the search credential and any trusted internal hosts. The managed [Web workflow](../guide.md) prompts for a credential and creates the ordinary tool definition.

### By hand

[`lnk agent tool add web`](../../../../../../../agent/docs/agents/README.md#add-tools-and-skills) is these three steps: the module, the tool
with its key, and your agent allowed to call it. Set `BRAVE_API_KEY` to
your key in the environment you run these from, and `--env BRAVE_API_KEY`
hands it to the tool.

```sh
lnk mod add web-tool
lnk sandbox tool add web --add fetch --add search --env BRAVE_API_KEY -- lnk-web-tool serve
lnk agent permission add --tools.names web
```

### Choose a Search Provider

For a direct registration, `LNK_WEB_SEARCH=brave` or `tavily` selects
the API and requires its corresponding credential. Pass that variable
with `--env LNK_WEB_SEARCH` as well as the credential variable. Without
an explicit selection, the provider uses Brave’s credential first, then
Tavily’s. Managed agent setup registers only the credential it selects.

## Fetch a page on your network

A page on this computer or your network is never fetched, so the tool
can't reach what your agent's sandbox keeps out. Name a host to let it
through, with `BRAVE_API_KEY` set as before.

```sh
lnk sandbox tool add web --add fetch --add search --env BRAVE_API_KEY -- lnk-web-tool serve --private-host wiki.home.arpa
```

`--private-host` takes a name or an address, and can be given more than
once. Adding the tool again replaces it, so give its key again too.
