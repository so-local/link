# Tools and Asking

> Tools Go Out, Your Logins Stay Home

[Tools run outside every sandbox](../security.md), and the sandbox asks you before a tool's
call or a page goes through. A call that asks
waits for your answer, and you list and give answers in [Sandbox
Questions](questions/README.md). You see what a tool's server offers, by
the names you allow, in [Tool Calls](tool-calls/README.md). List your
configured tools and their approval requirements with one command.

```sh
lnk sandbox tool list
```

## Tools that run outside

`lnk sandbox tool add`, then `--tools.names`, as in [`lnk agent permission add --tools.names`](../../../../../../agents/agent/docs/security/sandbox/permissions/README.md#choose-what-your-agent-can-reach): an MCP
server you add runs outside every sandbox, with your logins. That's its
point, and what it can do is yours to choose.

### Tool Definition

`~/.lnk/safe/sandboxes/tools/<name>.json` (600), read by the proxies outside,
never through a link: its command, a snapshot of a few variables
of your environment, and `--env`, which may hold a token. Every value of
its environment is held as a secret where Link reads it (never printed,
zeroed when dropped), written as it is only in that file, and given to
the tool's server alone, in its environment.

### What a Tool Runs With

Your `PATH`, `HOME`, user, locale, `TMPDIR`, SSH agent and desktop
session (`DISPLAY`, `WAYLAND_DISPLAY`, D-Bus) as they were when added,
and no more. A tool saved with any other field is refused.

### Where a Tool Runs

A tool runs in `/`, never the folder it was added in, which a sandbox may
write: an argument written from there (`.`, `./`, `../`) is refused,
and `lnk sandbox run` refuses a relative path from `/`. Its files, with
what their interpreter loads beside them (`node_modules`), go in a
folder no sandbox writes, which Link can't know for you.

### Reaching a Tool

A sandbox reaches only the tools its specification names, at
`/_link/url/<secret>/mcp/<name>` on its proxy (`$LNK_TOOL_<NAME>`), the
secret its proxy's, never logged: one request per connection, a body of
8 MB at most, one JSON-RPC message, no batches. Its proxy's `mcp` route
hands each to the sandbox module's bridge (`lnk-sandbox
bridge`), outside every sandbox, which runs the tools and keeps each
sandbox's sessions apart.

### Sessions

Each session a client starts is a server of its own; a sandbox keeps at
most 8 of each tool (a ninth ends that tool's least recently called,
never another tool's), a session with no call for 5 minutes ends, its
server with it, and at most two start at once. The rest wait their turn, and one still
waiting after 60 s gets a 429. A session however slow to start is
kept for its next call.

#### Session Settings

`tool_sessions`, `tool_idle` and `tool_starts`, read from Link's
settings the bridge is started with, never from the home a proxy runs
with, which for an agent is its harness's.

#### Client That Closes

A client that closes its connection before its session has started gets
none: the server is stopped and holds no place. One that closes it
before a call is answered lets the call go, and its session stays. On
Linux, one that only shuts its writing after its request (a half-close)
hasn't closed it, and is answered; on macOS, whose proxy is reached over
TCP, where the two look alike, that counts as closing it.

#### Tool Added Again

A tool added again with another command, folder or `--env` (a token
replaced) ends the sessions started before: their next request is
answered as gone, and the client's next session runs as the tool is
now. Removing a tool ends its sessions: the next call is refused.

### What passes between them

The client's own capabilities are removed from `initialize`, and what
the server asks of the client is refused outside, so no server reaches
the sandbox's folders or a model through it. Its notifications stay
outside. Listing its tools, prompts and resources passes as it is.

### Calls that ask

Each `tools/call`, `resources/read` and `prompts/get` goes through only
if the tool's rules allow it (`--add` with the call's name, or with
`resources/read` or `prompts/get`; or `--ask none`). A server's hints,
such as MCP's `readOnlyHint` or `destructiveHint`, are shown and never
skip asking. Otherwise the bridge asks you.

### Messages Without an ID

A message without an id reaches the server only if it's one of MCP's own notifications
(`initialized`, `cancelled`, `progress`, `roots/list_changed`): a
`tools/call` sent as one, which some servers would run without an id,
is refused.

### Logged

Each request is a line in the wire's log (`mcp`, the tool and a call's
name, never its arguments or answer), and refused calls count toward a
pause.

### Tools at a URL

`--url` is a stdio server of Link's own, `lnk-sandbox
mcp-remote`, run outside like any other. The server reaches `https://`, or
`http://` on this machine only, and sends each message as streamable
HTTP's POST with the server's session. Its headers, a token among them,
are in its definition's environment (`LNK_MCP_HEADER_<n>`), never in a
sandbox or on a command line.

## Shared Tools

`--shared` runs one server for every session of every sandbox naming
it, under a hub, `lnk-sandbox mcp-shared`, which the first
bridge to need it starts outside every sandbox. The bridge still asks
its tool's rules before a call reaches the hub. The sessions share one
server, and whatever it keeps between calls, so share only a tool meant
for everyone who names it.

### Hub Socket

The hub listens on a Unix socket in Link's settings
(`sandbox/tools/<key>.sock`, 600), which no sandbox reaches,
`<key>` signed with a secret this machine makes once and keeps
(`sandbox/tools/.key`, 600) over its name and what it runs as, its tokens among
it, so the name, which every user of the machine can list, tests no
guess of a token.

### Each Session's Answers

The hub passes each session's messages on with ids of its own, so an answer
goes only to the session that asked. Each session's answers are written
on a thread of its own, so one that stops reading holds up no other, and
one with 64 answers unread is ended.

#### What the hub says

What the hub says to the server (a ping's answer) is written apart from
the thread that reads the server, so a server writing while it reads
nothing never stalls it.

### One hub

A lock beside it keeps it to one hub; a tool defined again gets a hub of
its own, and the old one stops once idle. A hub that stops idle kills
its server, and what the server started, never only closing its input.

### Hub Folder

The bridge hands the hub Link's settings folder (`--dir`), which it
takes only if it's a folder of the user's alone, not a link, so a hub
started for an agent's tools listens, reads the definition and keeps
`shared_idle` there, never in the harness's home.

### Hub Unit

Where systemd runs Link's services, the hub runs in a user unit of its
own (`lnk-tool-<name>-<key>`), so no agent's stop ends it or counts it
as its own; elsewhere, in a process group of its own.

#### What the unit is handed

`HOME`, `PATH`, `LANG`, `XDG_CONFIG_HOME` and Link's `LNK_*` variables
by name, never on `systemd-run`'s command line, and none whose name
holds `TOKEN`, `SECRET`, `KEY` or `PASS`; what it's handed is in the
unit's environment, which your user and root can read (`systemctl
--user show`). Memory's service unit is handed the same.

### Hub Log

Beside its socket, the hub keeps at most 1 MiB of its server's output while
it runs, and a log past that when a hub starts is kept once as
`<key>.log.1`.

## Asking you

The sandbox's bridge asks for each route that asks you (a tool's call, a
page to open), one question at a time, and allows that call once. The
first answer counts, from whichever way it was asked, and no answer in
time is no. Unanswered, the refusal names the `--add` that lets it
through. A question waits as long as the agent that runs the sandbox
says (`[connections] ask_wait`, ten minutes in Link's specification), and for any
other sandbox two minutes (`ask_wait`, at most a day, from Link's
settings). It's asked every way there is at once:

1. A dialog.
2. Its proxy's controlling terminal.
3. A waiting question.
4. For an agent's sandbox, the owner's chats.

### Dialogs

macOS's `osascript`, passed the text as an argument, never as script; or
Zenity's text viewer or KDialog, followed by an empty answer form. They get the
desktop session's variables that `wrap` passes the proxy (`--desktop`),
and its route the bridge, never the sandbox. Answered another way
first, the dialog's program is stopped, which closes it, and a click
after then is no answer.

#### Where Dialog Programs Are Found

Dialog programs, a notification, and your browser are found in the system's folders
(`/usr/bin`, `/bin`, `/usr/local/bin`), never on the program's `PATH`,
which can name a folder the sandbox writes.

### Proxy Terminal

With a random six-character response code to type back: the proxy answers the
bridge's waiting question there. Anything else refuses it.

### Waiting Questions

A qualifying answer is returned in full to the requester and requires clarification before dispatch. Existing-policy calls keep their host binding and execution outcome tracking without adding questions. A source observer continues recording a dispatched call if its client disconnects; an unknown outcome cannot become a completed stop or a new execution claim.

### Shared Decisions

The owner's protected store is `~/.lnk/safe/approvals/`. Every surface, including a native dialog shown alone, submits through the same engine. A request has a permanent ID and a separate expiring response code. `lnk sandbox question list` displays full review and the code for `lnk sandbox answer`. The [interaction store](../../../../../../../packages/interactions/docs/more/libraries/interactions/interactions.md) serializes the first valid response, single-use dispatch claim and source outcome.

### Owner's Chats

For a sandbox an agent's harness runs in, the agent's bridge sends the
question to its owner's direct chats, with its separate code to reply
with: the code alone allows it, `no` and the code refuses it. The
[agent's bridge](../../../../../../agents/agent/docs/agents/more/interface/bridge.md#questions-on-your-chats)
says where it asks.

#### Only while someone can answer

A tool's call that nobody but a dialog would see is recorded before showing that dialog and refused at once if no surface is available. Otherwise it waits while a proxy's terminal or its agent's owner chats can answer. Kernel-locked presence records track these routes without holding the interaction transaction lock while the owner considers the request.

### What a Sandbox Can't Do to a Question

Nothing in a sandbox draws on the screen or reaches Link's settings, so
it can neither fake a dialog nor answer one or a waiting question. It
never sees a question's code either, so it can't answer one on a chat.

#### Questions on a Terminal

With no desktop or over SSH, the program shares it: it can type (macOS)
but not read what's written, so it can't answer, but it can write over
the question's text, so what the question names can be faked there. The
code you type still allows only the call that asked.

### Question in Full

A dialog, a chat or `lnk sandbox question list` shows the complete bound review. Control characters and invisible directional characters are rendered visibly. Details are never truncated to enable an authorization. A surface that cannot carry the complete review is refused rather than asking the owner to approve hidden scope. Tool-server hints remain labeled as unverified claims.

## Namespace servers

A host can expose existing local MCP servers as one namespace using a
private `Namespace` definition. It chooses every action's targets; callers
supply arguments and a `kind` selector, never a server name or address.
The namespace derives its discovery schemas from the original servers.
An action says it only reads (MCP's `readOnlyHint`) only when every
original tool it reaches says so, so a caller may run it beside other
reads. Any other action says nothing, and is taken as a change.

### Instructions

An MCP server's `initialize` answer may carry `instructions`, its
guidance to the model on using its tools. The namespace's own
`initialize` keeps each provider's, under the `kind` values that reach
it, since a provider's guidance names its own tools, which the namespace
serves as its actions.

### Keepalives

An MCP `ping` confirms that the namespace is responsive without starting
providers or dispatching actions. It does not grant access or bypass
the approval checks on subsequent tool calls.

### Unavailable Providers

The namespace opens a provider when a listing or call first needs it,
waiting at most 20 seconds, or less when its definition's timeout is
shorter. A provider that doesn't answer is left out of that listing,
and the next listing or call that wants it tries it again, so a provider
added or repaired later appears without reconnecting. A call that
reaches a provider still unavailable fails, naming the provider and why.
A harness that logs failed calls, as Link Harness does, puts that reason
in the agent's log, where [`lnk agent log show`](../../../../../../agents/agent/docs/agents/start/log/README.md#read-the-log) shows it.

### Selection

An action with several targets requires `kind`, unless the host marks it
as a broadcast read. A broadcast joins each target's result under its
kind. A mutation selects one target, retaining that server's own tool name.
Such an action is offered as one object schema, since models' APIs refuse
a union at a schema's top: every target's properties, `kind` naming each
target, and required only what every target requires. Each call is still
checked against its chosen target's own schema.

### Approval and lifetime

Every call reloads its provider definition and checks its current approval
policy. A removed provider is refused; a changed launch requires a fresh
session. Closing the namespace ends its owned sessions. Its host definition
sets a timeout, and request and answer sizes keep the tool bridge's cap.

### Argument checks

A namespace checks call arguments against the discovered provider
schema, rejecting unlisted fields before forwarding or approval.

<!-- docs-tree:start -->

## Docs

- [Tool Calls](tool-calls/README.md): A tool is an MCP (Model Context Protocol) server that runs outside the sandbox, and each thing it can do is a call
- [Sandbox Questions](questions/README.md): Some calls ask you first before the sandbox lets them through

<!-- docs-tree:end -->
