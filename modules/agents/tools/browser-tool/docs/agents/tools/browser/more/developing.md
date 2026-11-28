# Developing

`lnk-browser-tool` is Link Browser: an MCP server over stdio whose
calls drive a headless Chromium, for any harness to reach as a tool. How
to use it: [Link Browser](../README.md).

## How it drives Chromium

It starts Chromium as `<sandbox> --files.read <its folder> --files.write <profile>
--internet.hosts <host>... -- /bin/sh -c 'exec chromium ...'`, the sandbox
being what follows `--` in `serve`, so [`lnk sandbox run`](../../../../../../../../security/sandbox/docs/tooling/sandbox/guide.md#sandbox-guide).

### Hosts and program

The hosts are those `--internet-host` names, the agent's `[internet]
hosts`, and none gives no internet. The program is the one `--chromium`
gives, which the agent module passes from the specification's `[tool.browser]
program` after checking it (`link-browser-tool-contract`'s `program`),
else the one found on `PATH`. It's Chromium's own, links resolved, and
its folder alone is read, never the folder a link to it sits in. It refuses a Chromium older than `minimum_major` in
`src/browser.toml`, read from DevTools' `Browser.getVersion`.

### DevTools pipe

Chromium's
DevTools speak over a pipe (`--remote-debugging-pipe`, descriptors 3
and 4), which the shell maps onto the sandbox's own input and output,
since `lnk sandbox run` passes no other descriptor: one JSON message
each way, ended by a NUL byte.

### Its proxy

Its proxy is the sandbox's
(`--proxy-server="$HTTPS_PROXY"`), which leaves through the agent's route
when it has one (`--upstream`, as `run`'s calls do); a loopback address skips it and meets
the sandbox's wall, which opens only the ports `--private-host` names.
On Linux it always runs with `--no-sandbox`: Chromium's own sandbox
needs a user namespace, which Link's sandbox refuses inside, so it would
stop with "No usable sandbox!". Elsewhere it does so as root, where
Chromium refuses its own sandbox.

### Chromium's flags

It runs with `--disable-extensions`, and with `--use-mock-keychain` and `--password-store=basic`, so saved
sign-ins stay in its profile rather than a system keychain its sandbox
doesn't reach.

### Pages by conversation

Each call's `_meta` names its conversation (`link.local/thread`) and its
standing (`link.local/standing`), at most `--floor`, the agent's floor,
or `--floor` itself when it says none. Each conversation and standing
has a tab of its own (`src/tab.rs`): an owner's in the default browser
context, the profile's, and any other in one made with
`Target.createBrowserContext`, which keeps nothing on disk and refuses
downloads as the default one does.

### Closing and refusing

`close` closes the tab and disposes of its context, and the browser
quits with the last tab. A call whose `_meta` names no conversation is refused (`Who::of`). Before each call,
pages unused for `idle_ms` close, and past `pages_max` the least recently
used closes (`evicted` in `src/lib.rs`, both in `src/browser.toml`).

### Events and dialogs

A thread reads the DevTools pipe (`src/cdp.rs`). It keeps only the
events the tool reads (`events` in `src/browser.toml`), and only those of
a tab it watches: a subframe's loads, and tabs no page opened, are
dropped as they come. It keeps at most `events_bytes` of each page's,
the oldest dropped past it, so one page never pushes out another's
events, and drops an event larger than that.

### Dialogs

The thread answers a page's dialog as it opens, since a dialog holds the page and a click's
own call until answered. An alert gets OK, and any other dialog gets
what the conversation's last `dialog` call said, once, or is dismissed.
The paused-wire check reads the sandbox's pause list for `run-<pid>`,
the name `lnk sandbox run` gives itself.

### Reading and working a page

A page is read by a script (`src/page.js`) run in an isolated world,
one made with `Page.createIsolatedWorld`, which shares the page's
document but none of its scripts' variables. The script numbers each
part that can be clicked or typed into and keeps the parts by number in
that world (`__linkParts`), so the page can neither see nor forge them,
and parts in shadow roots and same-site frames are found too.

### Clicks and typing

A click
is the mouse's own events at the part's middle, scrolled into view and
offset through its frames (`src/place.js`), after `elementFromPoint`
shows nothing else on top there. Typing is the browser's text input, so
a page sees what a person's would send.

## Build and test

From the repository root, run `cargo build -p link-browser-tool`, then `cargo test -p link-browser-tool --test browser`.

### `tests/browser.rs`

`tests/browser.rs` drives it against a site served on this machine,
named with `--private-host localhost:<port>`, in `lnk sandbox run`: a
form, conversations' pages and sign-ins, and a page of dialogs, frames,
shadow roots, a covered part and hidden text. It
finds Chromium at `$CHROMIUM`, on PATH or in Playwright's folder
(`$PLAYWRIGHT_BROWSERS_PATH`), and reports an explicit skip without one. No
test reaches the internet.

## Direct Tool Setup

Manual Browser registration supplies its private profile, sandbox command, and permitted operations. The managed [Browser workflow](../guide.md) prepares those inputs for the agent.

### By hand

[`lnk agent tool add browser`](../../../../../../../agent/docs/agents/README.md#tools) is the `browser-tool` module, the tool shared by every
session of the agent with its calls allowed, and your agent allowed to
call it. The sandbox command after `serve`'s `--` is `lnk sandbox
run`: the tool adds Chromium, its folder of helpers, and the shell that
starts it, each named, so the sandbox runs nothing else.

```sh
lnk mod add browser-tool
lnk sandbox tool add browser --shared --add open --add read --add click --add type --add select --add scroll --add screenshot --add dialog --add key --add back --add hover --add close -- lnk-browser-tool serve --profile ~/.lnk/data/agents/main/browser --floor owner --internet-host '*' -- lnk sandbox run
lnk agent permission add --tools.names browser
```

`--floor owner` lets a call that names no standing use the profile's
sign-ins; leave it out for an agent others reach. `--internet-host '*'`
lets it reach any host. The browser's sandbox reaches the internet on
ports 443 and 80 only, since it names no other endpoint, and a page on
another port fails to load.

## Open a page on your network

Name a host to let it through. A host of your network is given as a
name, an address or a range; a port of this computer as
`localhost:<port>`.

```sh
lnk sandbox tool add browser --shared --add open --add read --add click --add type --add select --add scroll --add screenshot --add dialog --add key --add back --add hover --add close -- lnk-browser-tool serve --profile ~/.lnk/data/agents/main/browser --internet-host '*' --private-host nas.local --private-host localhost:3000 -- lnk sandbox run
```
