# Harness Endpoint

How Link carries the messages of a harness that speaks no connection
itself: its adapter names its endpoint, where it speaks the [Agent Client Protocol (ACP)](https://agentclientprotocol.com), and each message
from its connections is a prompt there.

## `lnk-<name> endpoint`

For a harness that speaks no connection itself: a JSON string, the
address of its ACP on this machine's loopback
(`"http://127.0.0.1:18800/acp"`,
`link_connection_contract::acp::ACP_PATH`), at one of its
`Launch.ports`, `LNK_PORT` taken into account. Read by `lnk-agent` in [`lnk agent start`](../../../../../../../../agent/docs/agents/README.md#create-an-agent), `connect`, `use` and `status`. A harness that speaks
ACP over stdio is served there by the [hub](../../../../../../hub/docs/agents/harnesses/more/protocol/hub.md).

### What Link carries for it

Link then carries the connections whose modules carry messages
(`describe`'s `bridged`: all four of Link's) for it. An adapter without
the command speaks its own connections.

## Each message's WebSocket

Each message is a WebSocket opened at the endpoint, upgraded on the
connection that proved it's the harness, with `Authorization: Bearer
<endpoint_key>`, `Link-Agent: <agent>` (`main`) and, when it has one,
`Link-Agent-Id: <its ID>`, one JSON-RPC message to a text frame:
`initialize`, then `session/resume` of the session its thread names,
then `session/prompt`.

### Its thread

The session's ID: `<connection>:<chat id>`: `telegram:123456789`, a forum
topic's `telegram:<group>.<topic id>`, `slack:D0123ABCDEF`, a channel's
Slack thread `slack:<channel>.<thread ts>`, `discord:<channel id>`,
`signal:15551234567`, and `link:<name>` from [`lnk agent chat send`](../../../../../../../../agent/docs/agents/chat/README.md#talk-to-it-here), Link's
own connection, sent the same way; `default` for none.

### The prompt

Its text is a text block, and its images, voice notes and documents
image blocks, audio blocks and embedded resources in base64, the
message alone, never the thread's history. The harness tells
`_link/taken` as soon as it has the message, before its answer.

### Its answer

The answer streams as `agent_message_chunk`s, each call it makes as a
`tool_call` titled with the tool's name (`memory__search`) and its end
as a `tool_call_update`, which the bridge shows where the agent's specification
says `[connections] activity`. The prompt answers once the turn ends. An
answer is read up to 16 MiB; past that the owner is told the agent
failed.

### `_meta.link`

What Link says beside a prompt is in its `_meta.link`
(`link_connection_contract::acp::Said`): `from`, `fromName`, `hops` and
`tools`, below, and a subagent's `parent`. One that isn't Link's, or
names what Link refuses, is refused before the model sees it.

## Proving it's the harness

With the contract's `proof`, each message goes on a connection of its
own that first proves it's the harness; a connection that doesn't is
sent neither the key nor the message.

### `GET /_link/proof`

At the endpoint's host and port, with `Link-Challenge: <32 to 128 hex
characters>` and `Link-Agent`, answered 200 with `Link-Proof`: the
HMAC-SHA256 of "link endpoint proof", a zero byte and the challenge,
keyed with `endpoint_key`, in hex
(`link_harness_contract::harness::endpoint_proof`).

### `TCP_NODELAY`

An endpoint that answers the proof sets `TCP_NODELAY` on the connections
it accepts: each message is a new connection, and without it a short
answer waits about 40 ms for the connection's delayed ACK. The
conformance kit reads it from the socket where the system says.

### Without `proof`

The key and the message go to whatever answers on the port, whether or
not it's the harness: nothing proves it is.

## Answering for many agents

With the contract's `agent`, the endpoint answers a connection as the
agent `Link-Agent` names, with that agent's key, so one process could
answer for many agents. Without `agent`, every one is its own agent's.

### By name

One naming no agent is its own agent's (`LNK_AGENT`), and one naming an
agent it doesn't run is refused (404) before the model sees it.

### By ID

Names are unique only to an owner: an endpoint answering for more than
one owner's agents finds the agent by `Link-Agent-Id`, refuses one whose
ID and name disagree (404), and one naming by name alone an agent two
owners share (409).

## Who a message is from

With the contract's `from`, a message from someone who isn't the owner
says who in `_meta.link`'s `from`, `fromName` and `hops`. The harness
records who it's from, tells the model who's speaking (never as the
owner), and passes the hops on to its tools; Link refuses a `from`
naming no such sender before it's sent.

### `from`

The sender (`link_connection_contract::is_from`,
`link_core::access::person`): `schedule:<id>`, `task:<id>`,
`agent:<id>`, `run:<id>`, a background command of `run`'s telling how
it ended, `member:<connection>:<user id>`, or `webhook:<name>`.

### A person or a webhook

A `member:` is someone in a chat the agent is shared with, as the
connection names its users (`member:slack:T0ACME/U0ALICE`,
`member:telegram:42`). A `webhook:` is a post to one of the agent's
webhooks, a stranger's.

### `parent`

A subagent's message (`from` `task:<id>`) names the conversation that
started it, as its scheduler keeps it with the firing. A harness that
keeps its work in a store of its own, as one behind its sandbox does,
records the subagent as that conversation's child before taking it: the
lineage the scheduler's delegation records in the machine's store.
Without it, such a harness refuses the subagent as having no lineage. A
`parent` beside any other sender is refused. It's new before Link's
first public release, so it changed with no migration.

### `fromName` and `hops`

A `member:`'s name on their connection, when it gives one: plain UTF-8,
at most 64 characters, no control character
(`link_connection_contract::is_person_name`). It's words for the model,
never who they are. `hops` is how many messages, each sent by an agent
on its own, led to it.

### Scheduler Calls

The scheduler's actions reach a harness in the `task`, `cron`, and
`agent` [namespaces](settings.md#namespaces), at `$LNK_TOOL_TASK`,
`$LNK_TOOL_CRON`, and `$LNK_TOOL_AGENT`. A call to one carries `_meta`
`{"link.local/thread", "link.local/hops"}`, the conversation it's made
in and the hops of the message being answered.

### Agent Tools' Addresses

A harness that keeps to the agent's folder's layout finds the agent's
own tools the same way: `LNK_RUN` for its run tool, `LNK_FILE` for its
file tool, and `LNK_CODE` for its code tool.

## A conversation's tools

With the contract's `tools`, a message may carry `_meta.link`'s `tools`
(`link_connection_contract::tools_of`): the tools its conversation may
use from then on.

### The list

The names the harness offers them by (`memory__search`), at most 256,
each 1 to 128 letters, digits, `_`, `-` and `.`, or empty for none.
One that isn't a list of names is refused before the model sees it.

### Kept with the conversation

The harness keeps the list with the conversation and offers it only
those of its tools from then on. A later list narrows it to the tools in
both, never widens it, and a message without one keeps it.

### `link.local/tools`

A call to its tools then carries `_meta` `"link.local/tools"` too, the
names of the tools the conversation has this turn, which the scheduler
reads so a subagent gets its parent's tools or fewer.

### `link.local/standing`

A call also carries `_meta` `"link.local/standing"`, the standing of the
message its turn answers: its sender's (`Said::from`), at most the
firing's (`Said::standing`), as `owner`, `member` or `stranger`. The
scheduler keeps it on what the call makes, so it never fires as more.

### `link.local/spec`

Link Harness's calls also carry `_meta` `"link.local/spec"`, the called
tool's own table of the agent's specification (`[tool.<name>]` over `[tools]`,
as JSON), for a tool with opinions of its own, such as Link Web's
`fetch` and `hosts`. The toolbox attaches that table to every call, an
empty one where the agent sets nothing; Link Web refuses a fetch
without it. It is independent of whether
the incoming message carried a narrowed tool list.

### `link.local/model`

Link Harness's calls also carry `_meta` `"link.local/model"`, the model
the conversation runs on, so a subagent the scheduler starts without a
model of its own runs on its parent's.

## A conversation's model

A message may carry `_meta.link`'s `model`, a model's name or an alias
of the specification's `[model.aliases]`, so its conversation runs on it from
then on: a subagent's, started on one. It's 1 to 128 letters, digits and
`-._:/@`, else refused. Link Harness keeps it as a [`/model:set`](../../../../../../../link/docs/agents/link/commands/models.md#switch-this-conversation) event of
the conversation, from the message's sender.

### A tool call's `activity`

Link Harness marks each tool call it streams with `_meta.link`'s
`activity`, `true` or `false`, where its conversation chose with
[`/chat:verbose`](../../../../../../../link/docs/agents/link/commands/chats.md#show-its-tool-calls). The bridge then shows the call, or doesn't, in place of
its specification's `[connections] activity`. A call without it is shown as the specification
says.

## A message that joins a running turn

A message that joins a turn already running is answered at once, its
prompt's answer saying `_meta.link.steered` true
(`connection::ask_for`'s `Asked::Steered`): the answer of the prompt that
started that turn answers it too.

### How the bridge marks it

The bridge marks the message as its specification's `steered` says, or not at
all; the message gets no answer of its own, since the turn's one
answer answers both.

### From `lnk agent chat send`

A message `lnk agent chat send` sends that joins a running turn is answered
by that turn's answer, which, its caller gone, the harness keeps for it:
the first kept in the thread after the message was sent, never one kept
before it.

## Answers after a crash

With the contract's `recover`, the request `_link/recover`
(`link_connection_contract::acp::RECOVER`) answers `{"answers":
[{"thread", "text"}]}`, on a connection opened as a message's is.

### What it hands over

Each answer of a turn it finished after a crash, or whose caller went
before it was told it (the bridge stopped), which nobody waited for,
handed over once (`connection::recovered`). The harness finishes the
crashed turns in a task of its own, so a bridge that stops waiting
leaves none half done.

### Two phases

Its params empty or `{}`, that is all. With `{"two_phase": true}`, each
answer also has an `"id"` (an opaque string) and is kept, handed over
again at each ask, until a request with `{"done": ["<id>", ...]}`, sent
once it went to the chat, lets those go. That one answers `{"answers":
[]}`, and an id that isn't one is passed over.

### When the bridge asks

The agent's bridge asks as it starts, again each time the harness
answers after it couldn't be reached, and every 30 seconds while it
runs, and sends each to the chat its thread is, but for a `link:<name>`
thread's.

### `lnk agent chat send`'s answers

The bridge leaves a `link:<name>` thread's answers kept, and `lnk agent
chat` asks for its thread's itself, at its start and before each
message, and shows them.
