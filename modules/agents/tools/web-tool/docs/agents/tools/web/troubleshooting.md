# Troubleshooting

What to do when your agent's [web tool](README.md) doesn't search or fetch a page. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk agent show
```

## Your agent has no `web__search`

The tool was added without a search key, so it offers only `fetch`.
Add the tool again with a key: [`lnk agent tool add web`](../../../../../../agent/docs/agents/README.md#add-tools-and-skills), and paste it.

## "search API refused the key"

The API rejected the credential. Check its validity and access to Brave
Search or Tavily, then add Web again with the corrected key. A rate
limit instead reports "too many searches for now": wait for the
provider’s limit to reset.

## "on this machine or its networks"

The page is local, on this computer or your network, which the tool
never fetches. `--private-host` names a host you trust.

## "didn't resolve" or "timed out"

The page's name took too long to look up, or the page too long to
answer: a fetch has 30 s for all of it. The error says which, and a
refused connection or a certificate the tool doesn't trust says so too.
Check the address, or try again once the site answers.

## "is application/pdf, not a page of text"

Link Web reads only text, and refuses a PDF or an image by the type its
server says, before any of it is downloaded.

## "came compressed as br"

The server compressed the page in a way the tool didn't ask for and
doesn't undo. It undoes only gzip and deflate. Fetch the page from a
server that sends it as it is, or in one of those.

## Each call asks you first

A manually registered tool may require approval for each call. Review
its definition with [`lnk sandbox tool list`](../../../../../../../security/sandbox/docs/security/sandbox/tools/README.md#tools-and-asking); the [manual setup](more/developing.md#by-hand)
shows how to register the provider with `--add fetch --add search`.
These flags belong to [`lnk sandbox tool add`](../../../../../../../security/sandbox/docs/security/sandbox/reach.md#call-a-tool-that-runs-outside), rather than
`lnk agent tool add`.
