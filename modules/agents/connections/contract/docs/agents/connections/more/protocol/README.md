# Interface

> Connection Calls and Delivery Contracts

Every connection module answers a fixed set of commands for `lnk agent`. This page lists those commands as `link-connection-contract` types them.

## Related Boundaries

[Owner questions](owner-questions.md) explains authenticated replies and
work controls on a connection. [Pushed messages](pushed.md) documents
hosted delivery, and [Agent Way In](way-in.md) defines authenticated
scheduler and inter-agent messages. The [connection policy](../../../../../policy/docs/agents/connections/more/protocol/policy.md)
defines common settings and narrowed-tool headers.
[Formatted answers](formatting.md) explains how each connection shows
an agent's Markdown.

## The contract

The contract is the `link-connection-contract` crate, [`connection.rs`](https://github.com/so-local/link/blob/main/modules/agents/connections/contract/src/connection.rs)
and the modules beside it. A module's `main` is the crate's
(`link_connection_contract::main`, `main_with_bridge`, `main_linked`), handed its `About` and the
`Operation` its command line read. An outside module needs only `MODULES` to plug in.

### Declared Once

The protocol's commands are declared once, in the contract crate's
[`actions.toml`](../../../../../actions.toml), with what each prints in
its [`types.toml`](../../../../../types.toml). Each connection lists the
ones it answers under `[implements]` in its own `actions.toml`, and its
command line is generated from that, so every connection reads its
command the same way. Unknown commands, flags, and extra operands are
refused before stdin is read. [`lnk agent connect`](../../../../../../../agent/docs/agents/connections/connect/README.md#choose-an-app) builds `connect`'s
command line from the same declaration
(`link_connection_contract::declared::ConnectionConnectArgs`).

### Answers as Declared

Each answer is written as its declared type, so one its declaration
refuses is an error, never printed. `sift` and `hold` print one of
their items a line, a `verdict` and a `held-line`, and `bridge` prints
nothing.

### Its Version

The connection protocol has a version, declared in the contract crate's
[`protocol.toml`](../../../../../src/connection/protocol.toml) as
`link_connection_contract::PROTOCOL`. Each line a module reads on stdin
that holds a `Bridge` (`bridge`, `link`, `owner`, `send`, `take` and
`receive`) and each answer of `connect`, `describe` and `rule` states it
as `protocol` beside its own keys (`link_connection_contract::stated`).
Its reader (`link_connection_contract::read`) takes `protocol` out first
and refuses a line that states none, or a version older than the oldest
it accepts or newer than its own, naming [`lnk upgrade`](../../../../../../../../../docs/more/modules/guide.md#upgrade).

### Unknown Keys

After `protocol`, its reader reads
the rest and refuses a key it doesn't know, so a newer field is never
dropped unread. A rule's own keys are the one exception: they're read
beside the secret's route, which takes the keys the rule doesn't name,
so the rule's writer is held to its declared shape instead.

## Connect and describe

Three commands every connection answers: one that makes the bot the agent's, one that says what the connection is, and one that says how a harness speaking it reaches its service.

### `lnk-<connection> connect --agent <name> [--taken <json>]`

Asks for the connection's token, from its `LNK_<CONNECTION>_...` variables
or else on the terminal, its questions on stderr. It refuses a bot in
`--taken` (`[{"id", "agent", "machine"?}]`, the bots other agents have,
here and running on boxes, `[]` when it's left out), and pairs whoever sends the bot its
one-time code. Read by [`lnk agent connect`](../../../../../../../agent/docs/agents/README.md#talk-to-your-agent), which stops the agent
meanwhile, keeps the settings and starts it again.

#### What it prints

It prints `link_connection_contract::Connected` on stdout, to another
program only, since its settings hold the token and a terminal would
keep it:
`{"settings", "bot": {"id", "name", "team"?}}`. `settings` is the
connection's section of `link_harness_contract::harness::Settings`, and
`bot.id` what another agent's `connect` refuses: a Telegram bot's id, a
Slack or Discord bot's user id, the agent's Signal number.

### `lnk-<connection> describe --json`

Prints `link_connection_contract::Describe`: `{"title",
"secret_fields"?, "linked", "bridged", "cli"?, "pushed"?, "held"?,
"hosts"?, "disconnect_hint"?, "share"?}`. Read by `lnk agent connect`,
which keeps it with the connection.

#### What each field says

`title` is its name as people write it, and `secret_fields` the fields
of its settings that are secret, which your secrets keep (as the agent's
own, by its ID, the connection and the field). `linked` says its account is linked
as a device (`link`, `state`), `bridged` that it carries messages
(`bridge`), and `cli` that it installs a program for a harness speaking
it.

#### How its messages come

`pushed` says its messages can be pushed over HTTPS (`hook`, `unhook`,
`sift`, `receive`), and `held` that they come only on a connection `hold`
holds. `hosts` names the hosts a harness speaking it reaches,
`disconnect_hint` what [`lnk agent disconnect`](../../../../../../../agent/docs/agents/connections/connect/security.md#you-and-your-shared-chats) says stays the user's,
and `share` whether a chat of it can be shared.

### `lnk-<connection> rule --json`

Prints `link_connection_contract::Ruled`, `{"rule"?}`: its `rule`,
left out when the harness holds the connection's secret itself, is
`{"field", "root", "host", "paths": [{"prefix", "rest"}], "header"?,
"refuse"?, "methods"?, "public"?}`. Read by `lnk-agent` with keys on the wire (`lnk agent secret
wire`).

#### What the rule says

`field` is the field of the connection's settings holding the secret,
and `root` the field a harness reads the service's address from. The
rest is the proxy's rule for the secret
(`link_sandbox_client::SecretRule`): `{secret}` where the secret goes
in a path's `prefix`, else in `header`; `rest` `name` or `path`;
`refuse` paths compared without case; `public` the separator ending the
secret's part that isn't secret.

#### How the harness is handed it

The harness is handed a placeholder in `field` and its proxy's
`secret_url` in `root`, and the proxy the secret. A module that can't
answer keeps the agent from starting, so its secret never reaches the
harness.

## Share a chat

A connection whose chats can be shared with the agent's owner's people
answers one more command, which waits for the owner to share a chat.

### `lnk-<connection> share`

A connection whose `describe` says `share` only. Reads
`link_connection_contract::Sharing` as one JSON line on stdin
(`{"settings", "code", "wait_secs"}`, the connection's settings with
its token, and a one-time code). Run by the core with the agent
stopped, which then writes the chat into the agent's specification's
`[access] message` (`link_agent_spec::with_message_access`, the rest of
the file kept).

#### What it waits for

It waits up to `wait_secs` for a message in a group of the connection,
from a user it's paired with (`allowed_users`), that mentions the bot
with the code as a word of its own, in any case
(`link_connection_contract::has_code`, constant time). It reads no one
else's message, and takes the code once.

#### What it prints

It says in the chat that it's shared, and prints `Shared` (`{"chat",
"name"?}`): the chat, a group the connection keeps, `chat:<connection>:<chat id>`,
anything else refused, and its name for people.

#### What `main` holds it to

`link_connection_contract::main` reads the line, caps `wait_secs` at
`SHARE_WAIT` (10 minutes), stops the module's own wait when it's over
(exiting nonzero), and refuses a chat of another connection. It never
prints the code or the settings, not even for a line it can't read. A
module hands it its way of waiting (`About::share`), which also makes
`describe` say `share`.

## A program in the harness's home

A connection whose `describe` says `cli` (`lnk-signal`) installs a
program a harness speaking it runs, and answers three commands for it.

### `lnk-<connection> cli install --home <dir> --json`

Reads its section of the settings on stdin (Signal's `{"account",
"allowed_users"}`), installs the program into that harness's home
`<dir>`, and prints `{"program", "state"}`: its path
(`<dir>/.signal-cli/signal-cli`), and what of the home is the agent's
account, `~/`-relative (`["~/.signal-cli/data"]`).

#### How the harness runs it

Read by `lnk-agent`, which runs it in the harness's sandbox, reaching
the adapter's hosts, before `configure`, and hands the adapter the path
in the section (`"signal": {..., "program"}`): an adapter runs that
program, and knows no layout of the module's.

### `lnk-<connection> cli state --json`

Prints `{"state"}`, as `install` prints it, with nothing installed.
Read by [`lnk agent move`](../../../../../../../agent/docs/agents/move/README.md#move-your-agent-to-the-cloud), `send`, `receive`, `backup` and `restore`,
which carry it with the harness's own state (`lnk-<name> state`) for
each such connection the harness speaks.

### `lnk-<connection> cli link --home <dir> [--again]`

Links the account in `<dir>` with that program, its QR code on the
terminal. It exits 0 once linked (`--again`: anew), and 10, [`NOT_LINKED`](../../../../../../../harnesses/portability/contract/base/docs/agents/harnesses/more/vocabulary/base.md#a-link-not-made), when it isn't and there's no terminal. Read by `lnk-agent` in place of the adapter's
`link` for such a connection, in the harness's sandbox.

## A linked account

A connection whose `describe` says `linked` (`lnk-signal`) links its
account as a device, and answers two commands for it.

### `lnk-<connection> link [--again]`

Reads the same `Bridge` as `bridge` does on stdin, and links the
connection's account as a device into its `state`, from a QR code on
stdout. It exits 0 at once when it's linked already (`--again`: links
it anew), and 10, [`NOT_LINKED`](../../../../../../../harnesses/portability/contract/base/docs/agents/harnesses/more/vocabulary/base.md#a-link-not-made), when it isn't and
stdout is no terminal. Read by
`lnk-agent` in `lnk agent connect` and [`lnk agent start`](../../../../../../../agent/docs/agents/README.md#start-your-agent), for a harness
with an `endpoint`.

### `lnk-<connection> state`

Prints `link_harness_contract::harness::State` as JSON: what in its
`state` folder is the link, `~/`-relative to that folder (Signal's
account), never a program it installed. Read by `lnk-agent`
(`lnk agent state-send`), which carries those paths as
`connections/<connection>/...`, named in the move's manifest (`connections`).

## Carry messages

For a harness that speaks no connection itself, the connection carries
its messages to the harness and its answers back, and sends what nobody
asked for, such as a schedule's answer.

### `lnk-<connection> bridge`

A connection whose `describe` says `bridged` only. Reads
`link_connection_contract::Bridge` as one JSON line on stdin, then
carries messages until stdin closes: each direct message from an
allowed user goes to the harness's `endpoint`, and the answer comes
back. Read by `lnk-agent` (`lnk agent bridge`, and `lnk agent start
--foreground`), for a harness with an `endpoint`.

#### The line it reads

`Bridge` is `{"agent", "agent_id"?, "identity"?, "endpoint", "key",
"proof"?, "parts"?, "model", "settings", "state", "spec"?,
"shared"?}`. `identity` is the agent's (`{"agent", "owner",
"teams"?}`), its ID sent as `Link-Agent-Id`, and `agent_id` too, for a
module from before `identity`. `settings` is the connection's section,
and `state` a folder for this connection alone, mode 700, which no
harness reaches.

#### The chats it's shared with

`shared` holds the ids of the connection's chats the agent is shared
with (its specification's `[access] message`, `chat:<connection>:<id>`), sent only
to a harness whose contract says `from`, and none when left out. Telegram, Slack, and Discord also use it to admit messages from members
of those shared chats, retaining their sender identity.

#### The agent's opinions for it

`spec` is the agent's opinions for the connection, `{"working"?,
"stream"?, "activity"?, "pace"?, "steered"?}`, its specification's
`[connection.<name>]` over `[connections]`: a reaction while it answers,
whether the answer is shown as it's written, with each tool it calls
above it as it calls it (`activity`, from its Agent Client Protocol (ACP) `tool_call`s), and
how often. Left out, the bridge's own way.

#### How a tool call shows

Link Harness titles a call with its name's words (`run command`), then
each argument its tool's `_meta` `link.local/summary` names, never one
marked `x-link-sensitive`. A bridge splits a `tool_call` with a `name`
into those words and the rest of its `title`, and writes
`activity_mark`, the words, then `activity_separator` and that detail,
cut at 200 characters. Any other `title` shows whole. `replace` drops
the lines from the finished answer, `keep` leaves them, and `off` shows
none.

#### How often it shows an answer

An answer is shown again never more often than the platform documents a
bot may write: Telegram once a second in a chat and every 3 seconds in a
group, Slack every 1.2 seconds. Discord documents none, so the bridge
follows the rate limits it's answered with.

#### The endpoint

A message goes to the harness's endpoint
(`link_connection_contract::ask`, which asks for the proof when `proof`
says the endpoint proves it's the harness first):
`http://127.0.0.1:<port>/...`, or over a Unix socket
(`link_connection_contract::UNIX_ENDPOINT`), `unix:<path>` for HTTP on
the socket at `<path>`, or `unix:<path>:<port>`, a sandbox's `in.sock`
told `<port>` inside first, two bytes, as its proxy would.

#### Who the socket may be

The proof, the message and a recovery all go that way. The socket is
reached only as itself, never through a link, and must be owned by its
folder's owner and listened on by that user, else it's refused.

#### Photos and documents

With `parts` (its harness's contract says `parts`; left out, no), a
message's photos and documents go with its text
(`link_connection_contract::holding` and `ask_holding`). Each whose
type is taken (`link_connection_contract::MEDIA`: PNG, JPEG, GIF or
WebP, PDF or text, and a voice note's Ogg, MP3, MP4 audio, AAC, WAV or
WebM, for a harness on ACP alone) and within `PART_MAX` (10 MiB) is
downloaded from its platform, at most `PARTS_MAX` (8).

#### How the parts are sent

Each part is checked to be what its type says, and sent as OpenAI's
`image_url` and `file` content parts, or on ACP as image, audio and
embedded resource blocks. Each one left out is named in a line of its
text, and why, and a message left with nothing is answered by the
bridge.

#### Without parts

Without `parts`, none is downloaded, and a message holding only them is
answered "I can read text only, for now."

### `lnk-<connection> send`

A connection that can send a message nobody asked for (`lnk-telegram`,
`lnk-slack`, `lnk-discord`, `lnk-signal`). Reads
`link_connection_contract::Sending` as one JSON line on stdin
(`{"bridge", "chat", "text"}`) and sends `text` to the chat. Read by
the bridge's way in (below), for an answer to a schedule or a subagent.

#### The chat it names

`bridge` is the same `Bridge` as above, and `chat` the chat as its
thread names it after the connection: `telegram:42`'s `42`, a forum
topic's `-1001234.7`, or a Slack thread's `C0123ABC.1700000000.000200`,
sent into that topic or thread.

#### Where it may send

It sends only to a chat of the owner's alone: on Telegram and Signal an
allowed user's own chat, on Slack the owner's direct conversation with
the app (`dm_channel`), on Discord the owner's direct messages
(`dm_channel`). An app or bot connected before Link kept those sends
nowhere until it's connected again.

#### A group it may send to

It sends to a group too, one an allowed user talked to the agent in
(any of its topics or threads), which the bridge keeps in `groups.json`
of its `state`, naming its schema
(`link_connection_contract::remember_group`), while the agent's specification
answers in groups: never with `groups = "never"`.

#### When it can't

It exits nonzero when it can't send, and 11 when the connection refused
the bot's token (`link_connection_contract::TOKEN_REFUSED`), which it
will until the token changes.

#### Through a linked connection's bridge

A linked connection holds one connection, its running bridge's, so
`lnk-signal send` hands the message to it at `send.sock` in `state`
(600): a bridge's call whose action is `{"chat", "text"}`, proved with
the `Bridge`'s identity and `key`, answered `{}` or `{"error"}`
(`link_connection_contract::send_through`, `take_sends`). The bridge
takes only its own agent's, and `send` fails when it isn't running.

<!-- docs-tree:start -->

## Docs

- [Pushed Messages](pushed.md): A machine that packs agents has a connection's messages pushed to it over HTTPS, or held for it, instead of a bridge polling them
- [Agent Way In](way-in.md): For a harness whose contract says `from`, `lnk agent bridge` listens at `~/.lnk/safe/agents/<agent>/way-in.sock` (600, `link_connection_contract::way_in`) for messages from others
- [Owner Questions on a Connection](owner-questions.md): A connection tells Link which of its chats are the owner's own, so the agent's bridge can ask the owner there what only they can answer
- [Formatted Answers](formatting.md): An agent writes its answers in Markdown, and each connection shows them in its chat's own formatting: bold, code, links, lists, and tables as the chat draws them

<!-- docs-tree:end -->
