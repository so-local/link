# Web Guide

Enable Web to fetch pages as text and optionally search with a Brave Search or Tavily account. The search credential stays with the tool, outside your agent’s sandbox.

```sh
lnk agent tool add web
```

## Add it

It installs `web-tool` if it's missing, takes the search key from
`BRAVE_API_KEY` or `TAVILY_API_KEY`, else asks for it (Enter for pages
only), and lets your agent call the tool. It restarts your agent with
the tool, and `--agent <name>` picks another agent. Your agent's first
[`lnk agent start`](../../../../../../agent/docs/agents/README.md#create-an-agent) offers it too.

### Ask it something

Ask "what's in the news in Lisbon today?" and it searches, fetches the
stories it found, and answers from them.

### What it costs

You need the `web-tool` module, which [`lnk mod add web-tool`](../../../../../../../../docs/more/modules/guide.md#add-or-remove-a-module)
installs. Fetching a page costs nothing. Searching takes a key from
Brave Search or Tavily, whose plans say what a search costs.


## Search with your key

In Link Harness, `/web:search` invokes the search action with its JSON
arguments and the running agent’s Web grants.
Link Web searches through Brave Search's API or Tavily's, with the key
in its own environment. Your agent and its harness never see the key,
because the tool runs outside the agent's sandbox.
Set `TAVILY_API_KEY` or `BRAVE_API_KEY` to your key in the environment
you run [`lnk agent tool add web`](../../../../../../agent/docs/agents/README.md#add-tools-and-skills) from, and the tool takes it from there.

### Which Search Provider

Managed setup selects Brave when both credential variables are set.
To select Tavily, run setup with only `TAVILY_API_KEY` set. Direct
[provider setup](more/developing.md#by-hand) can instead select the API
with `LNK_WEB_SEARCH`. Without a key, it offers only `fetch`. An agent whose
harness offers `search` all the same is answered that search needs a
key and how its owner gives one, so it can say so. A search gives
each result's title, address and snippet, and when it was published
where the API says.

### What a search asks

The agent can ask for up to ten results, or only those from the last
day, week, month, or year, with a query of up to 400 characters.

### Adding it again

Adding Web again with a new key keeps the hosts its tool was given with
`--private-host`. Without a new key it keeps the tool as it is, and says
it searches only when it already did.

## What a fetch gives

In Link Harness, `/web:fetch` invokes the fetch action with its JSON URL
and optional start offset, using the same Web grants.
A fetch gives a page as text your agent reads, whatever kind of page it
is, within limits of size and time.

### A page of HTML

A page of HTML comes as markdown: its headings, paragraphs, lists,
tables, code, quotes and links, each link whole so the agent can fetch
it next. A link is made whole as a browser makes it, against the
page's own `<base href>` where it has one.

### Marked as the page's

A page's title and text, and search results, comes inside a fence, an opening `<untrusted ...>` line and a closing line with the same random ID, which tells your agent's model that it's data to read and never your instructions, since anyone may have written it. Its note also tells the model to follow none of the page's instructions and never repeat them, nor a code word or link they ask it to pass on, saying only that the page held instructions it ignored. The tool's own notes, such as which part of a long page it is, stay outside. [Untrusted Content](../../../../../../../../packages/untrusted/docs/more/libraries/runtime/untrusted.md) explains the fence.

### Characters left out

A title or a search result's snippet never holds control characters or
the invisible ones that reorder the text around them (Unicode's
directional overrides), written as they are or as `&#...;` references.

### What's left out

Scripts, styles, images, menus, and forms are left out, and nothing on
the page runs. Text the page hides from a reader is left out too, as a
browser hides it: an element marked `hidden` or `aria-hidden="true"`,
or styled inline `display: none` or `visibility: hidden`.

### Text that isn't HTML

Other text, such as JSON or plain text, comes as it is, while a PDF or
an image is refused, by the type its server says, before any of it is
downloaded. A page sent as `application/octet-stream`, a type that says
nothing, is read by how it begins: text and HTML come as they would,
UTF-16 text included where it starts with its byte order mark, and
anything else is refused. A page is read up to 5 MB, and a fetch
that takes more than 30 s, its page's download included, fails.

### A compressed page

The tool asks for pages as they are. A server that compresses one
anyway, with gzip or deflate, has it undone, still up to 5 MB, and a
page compressed any other way is refused, naming how.

### A page's character set

A page in another character set, such as Shift_JIS or windows-1252, is
read in it, as a browser would: by its byte order mark, a few bytes at
its start naming the set, else the set its `Content-Type` names, else
the one an HTML page's `<meta>` names, else UTF-8. Its text, as UTF-8,
is still at most 5 MB.

### Many at once

A round's calls run at once, up to 16; a call past them waits until
one is answered. Two pages at a time are made markdown, as a page's
markdown takes many times the page while it's made, and a session's
fetches share one connection pool.

### A long page

A long page comes in parts of 24 KB, about 6,000 tokens, and each part
says where the next starts, before the page's text, so an answer a
harness cuts short still says it. The page is fetched for its first part, and
its next parts come from that copy, kept for 5 minutes (the last 3
pages), so reading a page through fetches it once.

#### `[tool.web] fetch`

Set `[tool.web] fetch` in your agent's
[specification](../../../../../../agent/docs/config/specs/agent.md) for larger or
smaller parts. Link Harness sends it with each call; a fetch that comes
without the agent's specification is refused, as [`[tool.web] hosts`](#toolweb-hosts)
says.

### `[tool.web] hosts`

Set `[tool.web] hosts` in your agent's
[specification](../../../../../../agent/docs/config/specs/agent.md) to keep its
fetches to the sites you name, narrower than the internet its sandbox
reaches:

```toml
schema = 1
[tool.web]
hosts = ["docs.rs", "*.wikipedia.org"]
```

Each entry names hosts as `[internet] hosts` does: a name, `*.` and a
name for every name under it, or an address. A fetch of any other host,
or a redirect to one, is refused before its name is looked up. A page
kept for its next parts is given only while its host is still named,
and a `--private-host` host must be on the list too.
Searches still run, but a result's page is fetched only if its host is
named.

#### Left out, and with no specification

Left out, as in Link's specification, Web fetches any public host. Link
Harness and `lnk agent web fetch` send the agent's `[tool.web]` with
each call, an empty table where it sets nothing. Web serves every agent
on the computer, so a fetch that comes without one fails closed: it's
refused, never made for any host. Through another harness, only search
works.

## Fetch a page on your network

Web refuses internal network addresses unless you name a trusted host. The [network setup](more/developing.md#fetch-a-page-on-your-network) explains that exception and how to retain your search credential when replacing the tool definition.

## Terminal Actions

`lnk agent web` exposes Web actions for the selected agent. Read an action’s JSON arguments from a file with `--request <file>`, or from standard input when the flag is omitted. The action uses the running agent’s configured tool and grants.

### Choose an Action

`lnk agent web fetch`, and `lnk agent web search` use the corresponding calls described above. Use `--agent work` to select another agent and `--json` for machine-readable output.

## Manual Registration

The [developer setup](more/developing.md#by-hand) explains direct Web registration, search credentials, and access to internal hosts.

## More about it

[Agents Security](../../../../../../agent/docs/agents/tools/use/security.md#web) has the
guarantees and gaps, and
[Agents Decisions](more/decisions.md#how-does-your-agent-reach-the-web)
why it works this way. To work on its code, see
[Developing Link Web](more/developing.md), and when it doesn't search or fetch,
[Troubleshooting](troubleshooting.md).
