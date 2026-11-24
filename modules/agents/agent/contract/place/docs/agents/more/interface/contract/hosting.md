# Accounts That Host Agents

The commands an adapter (`lnk-<kind>`) answers when its account hosts
agents. This package's `actions.toml` declares each, `agent-abort` to
`agent-team`, and an adapter whose account hosts agents lists them under
`[implements]`. Each reads one request as JSON on stdin and prints one
reply, as every adapter command does.

## Requests and Replies

The package's `types.toml` declares the request and the reply field by
field, and `link_agent_place::Request` and `Reply` are generated from
it. A key neither declares is refused, a field left at its empty value
is left out, and an empty `have` reads as one left out. `lnk` and its
adapters are released together, so the commands carry no protocol version of
their own.

## Whether it runs agents and which

The two commands [`lnk agent`](../../../../../../../docs/agents/README.md#agents) asks of every account: whether it runs
agents, and which.

### `lnk-<kind> agent-runs`

Optional. `{"runs_agents": true}`: the account runs agents itself, with no
box, and answers the commands below. An adapter without it (`has no command`) runs none, and the caller does not send it the hosting commands.

### Asked once

`lnk agent` asks each account once, and again once it's connected as
another, and keeps the answer (`~/.lnk/safe/agents/places/runs-agents.toml`): `lnk agent
list` asks `agent-list` only of an account that runs agents.

### `lnk-<kind> agent-list`

Reads `owner`; `{"agents": [{"id", "name", "owner"?, "state"}]}`, the
agents it hosts for that owner, `state` in lowercase words (`running`,
`paused`). A reply without `agents` says nothing of which it hosts, and
is never taken as none.

### Who reads it

[`lnk agent list`](../../../../../../../docs/agents/start/create.md#run-more-than-one-agent), [`lnk agent move`](../../../../../../../docs/agents/move/README.md#move-your-agent-to-the-cloud) (finding an agent there), and `lnk
secret delete` (signed in as the owner a move took an agent there as:
listed no longer, the agent took its keys with it, and the account is
no longer named).

## Create and Connect Hosted Agents

A host can create a blank agent with its defaults and connect its bot
without first creating that agent on this machine.

### `lnk-<kind> agent-create`

Reads `create: {"name"}` and the account; returns `agent: {"id",
"name", "owner"?, "state"}`. The host supplies the harness and model,
without creating an agent on this machine. `lnk agent create <name>
--on <account>` uses it only for an account answering `runs_agents: true`.

### `lnk-<kind> agent-connect`

Reads `agent` and `connect: {"connection", "token", "user"?}`; returns
`{"message"}` after the host saves the connection. Telegram is the
supported connection: `token` is its bot token and `user` its owner's
Telegram user ID when needed for pairing. The token travels on stdin.

## Moving an agent there and back

The commands a move runs on the account, each carrying the [Move
Stream](../../../../../../docs/agents/more/interface/contract/move-stream.md).

### `lnk-<kind> agent-in`

Reads `agent` (`{"id", "name", "owner"?}`) and `size` (its Home's and
memory index's bytes, when the machine it's on knows them); `{"move",
"have", "seal_to"}`, the move it starts and what it holds of the agent
already. Read by `lnk agent move <account>`.

### Models supplied by the host

`managed_models: true` asks the source to omit model and fallback
settings from its switch stream. Their credentials stay in the source's
secret keeper without a live agent grant. The same agent ID returning
there restores those settings; an explicitly removed key stays removed.

### Refused

`{"refused": {"why", "retry_at"?}}` says why it won't now, and when the
next move is allowed (Unix seconds), which `lnk` shows as it is, the
time as this machine's.

### `lnk-<kind> agent-out`

Reads `agent`; `{"move", "size"}`, the move out it starts and the bytes
it holds, or `{"refused": ...}`. Read by `lnk agent move` from the
account.

## Transfer a Move Stream

The receiving and sending commands exchange the selected phase and
validate the files that cross the account boundary.

### `lnk-<kind> agent-receive`

Reads its request as one line (`move`, `phase`), then a format-4 stream
of that phase on stdin, each file checked against its hash. A copy
answers `{"have"}`, what it holds then; a switch puts the agent in place
and answers `{"answers": true, "machine"}`, the agent running there, on
that machine. Read by `lnk agent move`, [`lnk storage push`](../../../../../../../../../storage/storage/docs/services/storage/guide.md#back-up).

### Phase `files`

With phase `files` and `agent` instead of a move, Home alone: it prints
Home's `{"have"}` as one line first, then reads a stream of
`files/...`, then prints `{}`.

### A conversation in a stream

A conversation is its folder, `files/chats/<folder>/`, as
`link_store_contract::store::folder` reads and writes it at every end:
`events.jsonl`, `conversation.md`, and `tools.json`, the tools it was
narrowed to, when a list narrowed it.

### Narrowed before its first event

One narrowed before its first event is a folder of `tools.json` and a
`conversation.md` holding its first line alone, which names its thread:
the folder's name can't, its `:` a `-`.

### A cloud that keeps conversations otherwise

It keeps each as narrow as its `tools.json` says, refuses a stream
whose list isn't one, and gives the same folder back on `agent-send`.
A conversation's narrowing is its `tools.json` and nothing else: a
folder whose file was removed moves with every tool, since nothing
tells it from one never narrowed.

### `lnk-<kind> agent-send`

Reads `move`, `phase`, `have` and, for a switch, `seal_to` (the key the
receiving `lnk agent receive` said); writes a format-4 stream of that
phase on stdout, but what `have` holds the same. With phase `files` and
`agent`, Home alone. Read by `lnk agent move`, [`lnk storage pull`](../../../../../../../../../storage/storage/docs/services/storage/guide.md#get-files-back).

### A switch

The complete switch stream is sealed to `seal_to`
(`link_secretive::stream`), including its harness and connection
files; `receive` authenticates its final record before success. A
switch pauses the agent there first, messages that come meanwhile held.

## Finish or Abort a Move

The source finishes only after the destination answers. A failed
preparation instead aborts its move and resumes the source.

### `lnk-<kind> agent-left`

Reads `move`; `{"held": [{"connection", "chat", "from", "text"}]}` once its
copy of the agent is deleted: the messages that came while it was
paused there, which `lnk agent deliver` gives it where it went. Read by
`lnk agent move`, once the agent answers where it went.

### `lnk-<kind> agent-abort`

Reads `move`; `{}` once a move in's copy is removed, or a move out's
agent answers there again. Read by `lnk agent move`, after any failure
before the other end answers.

## Backups and chats

The optional commands for an agent the account runs: its backups, and
a chat with it.

### `lnk-<kind> agent-backup`

Reads `agent`; `{"at"}` once the account has backed the agent up where
it runs, never stopping it (`at` in Unix seconds). Read by `lnk agent
backup` of a hosted agent. Optional: an adapter without it keeps no
backups of the agents it hosts, and the agent module's internal `lnk agent backup` says so.

### `lnk-<kind> agent-restore`

Reads `agent`; `{"at"}` once its last backup there is restored (its
conversations stay). Read by the agent module's internal `lnk agent restore` of a hosted agent.
Optional, as `agent-backup` is.

### `lnk-<kind> agent-chat`

Reads `agent`, `thread` (a `link` thread, `link:<name>`) and `text` (the
owner's message, at most a megabyte); writes JSON lines on stdout as the
answer comes (`link_agent_place::Chatting`). Read by [`lnk agent chat send`](../../../../../../../docs/agents/chat/README.md#talk-to-it-here)
of a hosted agent, which kills the adapter when it stops waiting
(Ctrl-C). Optional: an adapter without it hosts agents `lnk agent chat send`
doesn't reach.

### Its lines

`{"piece": "..."}` for each piece of it, then one end: `{"answer":
"..."}`, the answer whole (at most 16 MB), `"steered"`, the message
joined a turn already running, whose answer goes where that turn's
does, or `{"refused": {"why"}}`. An answer nobody waited for isn't kept
for the next chat: it's in the agent's conversation.

## Hosted teams

A hosting adapter may answer `agent-team`, taking `Request.team`
(`link_agent_place::teams::Action`) and returning `Reply.team`.
Unsupported operations fail as any optional adapter command does.

### Its operations

[`teams::Action`](../../../../../src/teams.rs) defines `list`, `get`,
`put`, `apply`, `start`, `stop`, `delete`, `invites`, `join`, `leave`,
`begin-in`, `begin-out`, `backup`, `restore`, and `folder`. `put` carries
the team ID, complete file, and version read; `apply` carries the desired
file and version for lifecycle application. The typed replies, invitation,
and movement descriptors live beside that action type. The `chat` action
uses the streaming command below.

### What a reply shows

An invitation exposes only the requesting agent's row. The owner's team
file reply also says which outside principals joined.

### A team message

`agent-chat` also accepts `Request.team` with operation `chat`, a team ID
and an optional member name, instead of `Request.agent`. The host resolves
the current lead when the member is omitted. Thread, text and streaming
answers use the same personal-chat fields and protocol. Unknown members
and a team with no selected lead are refused. This is owner chat with the
owner's agents; outside members retain their separate consent and scope.

### A team movement

A team movement contains the parent move ID, the complete team file and
each own member's ordinary move ID. Existing `agent-receive`, `agent-send`,
`agent-left` and `agent-abort` carry that movement. Each member may
answer `managed_models: true`, which retains and omits that member's
source models as an ordinary hosted move does.

### Its stream

The outer format-4 manifest names `team` (the TOML file);
`files/members/<agent>/<chunk>.part` carries each member's format-4
stream in ordered 64 MiB parts, and `files/shared/` carries the team's
folder. Hash and end checks apply at both levels.

### Its switch

The receiver prepares all members before one switch. Only the parent
movement can switch, finish or abort the team. A team exit returns
`Reply.team_held`, keyed by the receiving member's ID, so held messages
reach that member's conversations.

### Restored model credentials

On a team's return, each source model stays retained until the whole
team's durable finish receives the host's `agent-left` acknowledgment.
A failed member preparation can then roll back without losing another
member's restored source key. Explicit secret removal still revokes it.

### A team's folder

With phase `files`, `Request.team` set to `folder` and a permanent team
ID selects that team's folder instead of `Request.agent`. Push and pull
keep the existing Home-stream contract.

## Team Relay

A local member uses its own agent token to reach a team hosted elsewhere.
The relay carries member messages while enforcing current membership.

### A team's relay

A member that runs on a machine of yours reaches teammates elsewhere
over HTTP, through the team relay of the account that hosts its team
(`link_agent_place::relay`). It signs in with its own agent token,
granted `team.member` on `team:<ID>`, never its machine's.

### Send and pull

A member posts a `TeamSend` to `/v1/teams/<owner>/<team>/messages`: the team,
its `team-<ID>` conversation, the member it's for or none for each,
its hops and its text, never who sent it. The relay takes the sender
from the token, checks it is on the team at each message, and answers
`Sent`. A member pulls what was kept for it with `GET` on the same
route and `?after=<seq>`, its `Mailbox` holding at most 64 `Mail`,
oldest first. The relay lets go what it kept through `after`. The
scheduler is [this contract's client](../../../../../../../../tools/scheduler-tool/docs/agents/tools/scheduler/more/developing.md#the-team-relay).

### Where it is

`agent-team list` gives each `HostedTeam` its owner's account (`owner`)
and its relay's address (`relay`), each optional, so a host without a
relay leaves them out. The agent module starts a member's runner with
the relay of the first of its teams that has both.
