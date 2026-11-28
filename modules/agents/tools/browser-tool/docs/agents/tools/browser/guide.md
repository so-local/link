# Browser Guide

Enable Browser to open pages, fill forms, and inspect screenshots with a browser profile reserved for your agent. Browser works on Linux only today. Install a supported Chromium browser before enabling the tool.

```sh
lnk agent tool add browser
```

## Add it

It installs the `browser-tool` module if it's missing, defines the tool for this
agent alone, and restarts your agent with it; `--agent <name>` picks
another agent, and its first [`lnk agent start`](../../../../../../agent/docs/agents/README.md#start-your-agent) offers it too. `lnk
agent tool remove browser` takes it away; its profile stays.

### In its specification

[`lnk agent tool add browser`](../../../../../../agent/docs/agents/README.md#tools) also writes `enabled = true` under
`[tool.browser]` in the agent's specification, and `remove` writes `false`. An
agent whose specification says it, such as one imported from an archive, gets
its browser at its next start wherever it runs, [its module installed if
it's missing](../../../../../../agent/docs/agents/tools/use/decisions.md#why-can-a-specification-turn-a-browser-or-desktop-on).

### What it needs

You need the `browser-tool` module, which [`lnk mod add browser-tool`](../../../../../../../../docs/more/modules/guide.md#add-or-remove-a-module)
installs, and Chromium, Google Chrome, Microsoft Edge or Brave on this
computer, version 120 or newer. The browser [refuses an older one by
name](more/decisions.md#why-does-it-refuse-an-old-chromium), since it runs without Chromium's own sandbox on Linux.

### On a box or Ubuntu

Ubuntu offers Chromium only as a snap, a package that runs under
snapd's own confinement. That confinement doesn't start inside the
browser's sandbox, so on Ubuntu, including a box made from Ubuntu's
image, install Google Chrome's Debian package instead. Google builds
it for x86-64 machines alone. Once you're in the box with
[`lnk box ssh gcp-1`](../../../../../../../compute/box/docs/services/boxes/ssh.md#open-a-shell-on-a-box),
download the package there, then install it with
`sudo apt-get install -y ./chrome.deb`.

```sh
curl -fsSLo chrome.deb https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb
```

The browser finds it as `google-chrome-stable`. An agent that also
uses the [desktop tool](../../../../../desktop-tool/docs/agents/tools/desktop/guide.md#what-it-needs) needs
`xvfb` from apt too.

### Name its program

The browser looks for `chromium`, `chromium-browser`, `google-chrome`,
`google-chrome-stable`, `microsoft-edge` and `brave-browser` on your
`PATH`. It resolves the launcher
scripts Debian, Ubuntu and Google install to the program each starts.
Name the program yourself when the browser you want isn't among those,
or when it's started through a launcher script of its own, such as a
wrapper that sets flags. Name it in the agent's specification, by its absolute
path:

```toml
schema = 1
[tool.browser]
program = "/opt/chromium/chrome"
```

The browser then starts that program and searches for none, allowed in
its sandbox as a program it finds is. Its flag names it from the
command line, `lnk agent start --tool.browser.program
/opt/chromium/chrome`.

### Program, never a script

The program must be the browser's own executable, never a script: a
file starting with `#!` runs inside the browser's sandbox, which refuses
every program the script calls (`lnk sandbox: refused /usr/bin/id: add
it to [commands] allow`). Each start checks the setting and refuses a
relative path, a missing file, a file you can't run or a script by the
setting's name, and your agent then runs without its browser.

#### Find the Real Program

To find what a launcher starts, read its last lines, and check the
result with `file`, such as `file /opt/chromium/chrome`, which says
`ELF` for a Linux program.

### Ask it something

Ask "find me a table for two at Tasca do Chico on Friday, and show me
a screenshot before booking". The agent can open the site, read it,
click through, and fill the form. Browser grants allow these actions
without a separate booking confirmation, so include any approval step
in your request.

### Its profile

Its profile, with its cookies and sign-ins, is
`~/.lnk/data/agents/<name>/browser`, in the agent's folder, which its
harness never reaches.


## What it does

Your agent calls it as `browser__<call>`, a page at a time. In Link Harness,
the owner can invoke the corresponding slash command with its JSON
arguments, subject to the same enabled provider and grants.

### `open` and `read`

`/browser:open` opens a page by its URL, waits for it to load, and reads it.
`/browser:read` invokes the corresponding read action.
`read` gives the page's text, each part that can be clicked or typed
into numbered where it stands (`[3]`), 24 KB at a time.

### `click`, `type` and `select`

`/browser:click` clicks a part, by its number. `/browser:type` and
`/browser:select` invoke the corresponding input actions. `type` types into a field,
replacing what it held, and with `submit` presses Enter. `select`
chooses options of a drop-down, each by its text or value, in `values`:
one for a drop-down of one choice, and each given for one of many, the
rest unchosen, as Playwright's `selectOption` does. Playwright is the
browser automation library many models have learned to drive pages
with, so its shapes are the ones they reach for.

### `key`

`/browser:key` presses a key where the page's focus is, such as Escape
to close a menu or an arrow to move through a list. It reads the key as
Playwright's `press` does: a named key (`Enter`, `Escape`, `Tab`,
`Backspace`, `Delete`, `Space`, the four arrows, `PageUp`, `PageDown`,
`Home`, or `End`), or one character (`a`, `A`, or `/`), after any of
`Control`, `Alt`, `Meta`, and `Shift` joined by `+`, each held while
the key is pressed (`Control+a`, `Shift+Tab`).

### `back` and `hover`

`/browser:back` goes
to the page before, as a browser's back button does. `/browser:hover`
moves the pointer over a part, by its number, for a menu that opens on
hover.

### `scroll`, `screenshot` and `close`

`/browser:scroll` scrolls the page down or up by most of a window, or a part
into view. `screenshot` is an image of what the window shows, 1280
by 800 pixels, requested with `/browser:screenshot`. `/browser:close`
closes this conversation's page, and the browser with the last one. Its
profile stays.

### Dialogs

A page's alert is answered OK, which is all it offers. A `confirm()`, a
prompt or a question before leaving the page is dismissed, so a click
never deletes an account unseen, and the answer says what it asked.
`/browser:dialog` with `accept` set to true answers the page's next
question OK, once, with `prompt_text` for a prompt. The agent then clicks again.

### What a page does meanwhile

The numbers are those of the last read, so after a page changes, read
it again. A link that opens a new tab makes that tab the page, and the
old tab closes. When one click opens several tabs, the last is the page
and the others close. A click on a part that something else covers, such as a
cookie banner, is refused, and the answer names what covers it. After a
click or typing is sent, a later failure comes back as a note rather
than an error, so the agent reads the page before sending it again.

### What a read holds

A read holds the text a person sees. Frames of the page's own site are
read where they stand, and another site's frame is named,
`[iframe: <address>]`. Text hidden from the eye isn't read: text that's
see-through, with no color, folded to no height, clipped to a pixel, or
placed off the page. Parts in a shadow root, a component's
own part of the page, are numbered and clicked like any other. Each
note a page writes, such as a dialog's words or a title, holds at most
2 KB, and the call's notes count against the read's 24 KB.

### Marked as the page's

A page's address, title, and text, what its dialogs say, and a download's name each comes inside a fence, an opening `<untrusted ...>` line and a closing line with the same random ID, which tells your agent's model that it's data to read and never your instructions, since anyone may have written it. The browser's own notes stay outside, so one a page forges is still inside. A screenshot can't be fenced, so a fence's note comes before it. [Untrusted Content](../../../../../../../../packages/untrusted/docs/more/libraries/runtime/untrusted.md) explains the fence.

## Its sandbox

The browser starts at the first call, headless, in a sandbox of its own, which [`lnk sandbox run`](../../../../../../../security/sandbox/docs/tooling/sandbox/guide.md#sandbox-guide) makes. It reads its own program, writes only its profile,
and reaches the internet hosts the agent's `[internet] hosts` name
through the sandbox's proxy. It reaches nothing else on this computer or
your network, however a page asks: a link, a redirect, or a script, but
the ports and hosts the agent's grants name, as [Open a page on your
network](#open-a-page-on-your-network) says.

### Internet hosts

The browser reaches exactly the hosts its agent does. An agent whose
grants name a few hosts browses only those, and one with none browses
nothing. To let the browser reach any site, grant every host
explicitly:

```sh
lnk agent permission add --internet.hosts '*'
```

### Paused way out

The sandbox's proxy pauses a sandbox whose calls look like no normal
work, such as a page asking for a cloud's metadata service. The browser
then says its way out is paused and names the command that opens it
again. It starts no new browser in its place until you resume it.

### Each conversation's page

Every session of the agent shares one browser, and each conversation
has a page of its own, so a page, what was typed into it, and `close`
stay in that conversation. A conversation answering you, the owner,
opens in the profile's own context, so its sign-ins last from one
session to the next and every owner's conversation shares them. Any
other turn, a teammate's, a member's in a shared chat, or a stranger's,
opens in a context of its own, which sees none of the profile's sign-ins
and keeps nothing past the browser. A member's turn in a chat you share
gets its own page, apart from yours.

### A shared agent's sign-ins

An agent you share, in a chat with members or on a team, uses no
sign-ins at all: every conversation, yours included, opens in a context
of its own. Its browser can't tell your turn from a member's with
enough certainty to hand it the profile, so it hands it to no one. To
use the sign-ins, keep the agent to yourself.

### How many pages stay open

A call that doesn't name its conversation is refused, as one page
would otherwise serve every conversation. Link Harness and mini name
it. At most 16 pages stay open, and opening one more closes the page
used longest ago. A page unused for 30 minutes closes too. A
conversation whose page closed gets a new one at its next call, without
what it had open.

### Its profile across sessions

Managed setup always gives it a separate profile. Direct registration
refuses the known personal profile locations for supported browsers,
and refuses to start with no `HOME` to find them in. Choose a separate
folder if your own profile uses a custom location.

### Signing in

The only way to sign in is for the agent to `type` the password into
the page, so the password passes through the model, its provider, and
the conversation's logs. Give the agent only accounts you'd hand it,
or sign it in to a site that offers a separate account or token.

### Downloads and uploads

It downloads nothing and uploads no file: each is refused, and the
answer says so.

## Open a page on your network

The agent's browser reaches each port of this computer its agent's
`[localhost] ports` name, as `localhost:<port>`, and each host of your
network its `[lan] hosts` name, as it reaches its internet hosts. Grant
the one a trusted local site is on:

```sh
lnk agent permission add --localhost.ports 3000
lnk agent permission add --lan.hosts nas.local
```

Every port or every private range (`*`) names no site, so the browser
reaches none of them by it. A browser you define by hand names its own
[network configuration](more/developing.md#open-a-page-on-your-network).

## Terminal Actions

`lnk agent browser` exposes Browser actions for the selected agent. Read an action’s JSON arguments from a file with `--request <file>`, or from standard input when the flag is omitted. The action uses the running agent’s configured tool and grants. Each conversation has a page of its own, so the request names one in `thread`, such as `"thread": "link:main"` to work that chat's page; a request without one is refused.

### Choose an Action

`lnk agent browser open`, `lnk agent browser read`, `lnk agent browser click`, `lnk agent browser type`, `lnk agent browser select`, `lnk agent browser scroll`, `lnk agent browser screenshot`, and `lnk agent browser close` use the corresponding calls described above. `dialog`, `key`, `back`, and `hover` are the agent's alone. Use `--agent work` to select another agent and `--json` for machine-readable output.

## Manual Registration

The [developer setup](more/developing.md#by-hand) explains custom Browser definitions, including the shared profile and allowed operations.

## More about it

To work on its code, see [Developing Link Browser](more/developing.md), and when
the browser doesn't start or a page isn't reached,
[Troubleshooting](troubleshooting.md).
