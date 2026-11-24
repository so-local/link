# Decisions

Why connections pair an owner, isolate conversations and delegate delivery through a bridge.

## Connections

How a connection is set up, shared, carried for a harness that speaks
none, and how an agent's webhooks take their posts.

### Connecting a connection

Where a connection's setup lives, which a harness speaks, and who a
connection answers.

#### Where does a connection's setup live?

In a module per connection (`lnk-telegram`, `lnk-slack`, `lnk-discord`,
`lnk-signal`), installed the first time you connect it, rather than in
the harness module. You install only the connections you use, with what
each needs, such as Discord's Gateway client, and adding one doesn't
grow the harness module. The harness module keeps what's the same for
every connection: its settings, stopping the agent while it pairs,
`disconnect`, switching and moving.

#### Which connections does a harness speak?

Its adapter says, with `lnk-<name> connections`, so `connect` refuses one
it lacks and `use` says which stop answering. A connection stays in the
settings when the harness changes, so switching back brings it back. An
adapter that doesn't answer it speaks Telegram. An adapter with an
`endpoint` also speaks the connections Link carries.

#### Why does the harness module know no connection by name?

So a connection is a module like a harness: `lnk-matrix` on the PATH
plugs in, and adding one of Link's own touches only its module and the
core's list.

#### One map

An agent keeps one map of connections, each as its module's `connect`
made it and `describe` said of it (its title, which fields are secret,
whether it's linked as a device, its hosts), and hands each section to
the adapter under the connection's name. The typed sections of Link's
own connections stay in `link_harness_contract::harness`, the contract
the adapters read, and a connection from outside Link sits beside them.

#### One map for every connection

Settings and moves hold connections in one map, `bots`, and `lnk agent
list --json` prints it as `bots`. A Telegram token keeps its Keychain
name, `bot-token`, so no Mac asks for it again.

#### Who does a connection answer?

Whoever sends the one-time code printed in your terminal, in a private
chat or direct message (Telegram, Slack, Discord), or the number you
type (Signal), as its owner, and no one else as its owner: a chat you
share it with is answered as its members. Knowing the bot, or being in
the workspace, isn't enough. Slack's allowlist is by member id: the
harness's own for one that speaks Slack, the bridge's for one Link
carries.

### Shared chats

Why the owner shares a chat, and who in it may do what.

#### Why share a chat rather than a workspace?

The owner shares individual Slack connections, Telegram groups and
Discord connections. A member's direct message stays private to the
owner, and bots are refused. Each agent is the cordon: everyone in a
shared chat can use all its tools, files and memory. Separate files
belong to separate agents. The owner pays for every turn, with no cap
per chat.

#### Why keep the audience in access?

`[access] message` lists chats, each a group a connection keeps, never a
principal: `link-agent-identity`'s `Chat`, `chat:<connection>:<id>`. A
connection names a person as `member:<connection>:<id>`, including
the workspace when its user IDs are workspace-scoped. The harness keeps
the person's identity and name beside their message, never treating them
as the owner. Sharing requires a harness that supports these senders.

#### Who may approve a shared agent's actions?

The owner alone shares, unshares and approves sandbox requests. An
admitted member can use the agent's tools. A stranger receives only the
tools the specification gives strangers, on a harness that can narrow a
conversation's tools. The agent's own schedules, subagents and
background commands act for its owner.

#### Why does sharing stop the agent?

The connection module takes a paired user's mention with a one-time
code, within ten minutes, while the bridge is stopped. It never writes
the specification. The command atomically edits only `[access] message`,
preserving comments, then restarts the agent. Sharing is done on its
machine before a move to a cloud. The three connections use the same
contract and are released together.



### Carrying a connection

How Link carries the connections of a harness that speaks none, and
where that runs.

#### How does Link carry connections for a harness that speaks none?

The connection modules already make the bot and pair its owner; for a
harness that speaks the Agent Client Protocol (ACP) at its endpoint instead of speaking connections
(`lnk-<name> endpoint`), the module carries the messages too:
`lnk-<connection> bridge` sends each message from the owner there and the
answer back, streamed while it's written.

#### ACP

Any harness that answers one local endpoint gets every connection Link
carries, and a new connection reaches every such harness at once. ACP, is the shape, as every harness is driven the
same way so they differ as little as possible, a harness on stdio
through the hub; only the new message is sent, never the thread's
history, since remembering is the harness's.

#### Where does the thread go in the request?

In the session's ID, `<connection>:<chat id>`: each thread is an ACP
session of its own.

#### Where does the carrying run?

In each connection module, outside the harness's sandbox, beside the
agent while it runs: a background service of its own (`lnk agent
bridge`, which runs `lnk-<connection> bridge` per connection), stopped with
the agent, or a thread of `lnk agent start --foreground`; or, where the
machine says so, one bridge for every agent (`lnk agent bridges`).

#### What the bridge holds

The harness never holds the connection's token, and the bridge holds no
state but what the connection needs, so a harness answering elsewhere
sees no difference. The harness's endpoint takes a key Link makes at
each start, since other users of the machine can reach its loopback.

### Endpoint Identity

Why the bridge verifies a harness before sending credentials, and how
that proof works while connections start, wake, and move.

#### How does a bridge know the harness answers on its port?

The endpoint proves it: each message goes on a connection of its own,
which first answers a random challenge with an HMAC of it made with the
endpoint's key, and only then gets the key and the message.

#### Why a proof

A start checks the agent's ports are free, but another program can take
one in the moment after, or while the harness's service restarts, and a
bridge would hand it the key. The other way, the endpoint on a unix
socket in a 700 folder, keeps everyone else off it, but a harness's
endpoint is an address on the loopback in the contract, which an adapter
released earlier can't change, and an adapter can't always make its
harness listen elsewhere.

#### Additive

A challenge needs only a route and a function in the adapter, so it's in
the contract as `proof`, a field with no new version, which an adapter
says it answers: older ones keep working, unchecked. The proof is on the
same connection as the message, so the port can't change hands between
them, and it's a signature of the challenge, never the key, so asking
for one gives nothing away.

#### Waiting through a wake

An agent asleep for being idle is reached through its door, which holds
the connection while it wakes the agent and closes it if the agent
doesn't answer within two minutes. The bridge waits as long for the
proof. A wake that takes longer than a moment, on a busy machine, is
then answered by the harness, never taken for another program's answer
and the message dropped.

#### Which connections does Link carry?

Telegram, Slack, Discord and Signal, where people already chat: all four
for any harness, so any harness can answer anywhere Link connects. How a
long answer is split, and showing the agent typing, are each connection
module's, as their limits differ:

- Telegram's answer is edited in place about once a second and split
  past 4,096 characters.
- Slack's is the same, split past the 4,000 characters Slack
  recommends; Slack shows no bot typing, so 👀 goes on the owner's
  message while it's answered.
- Discord's is the same, split past 2,000 characters.
- Signal caps how often a message can be edited, so it shows typing
  and sends the whole answer, split past 2,000 characters.

#### Where does a linked connection's device live?

In a folder of the agent's for that connection,
`~/.lnk/safe/agents/<agent>/connections/<connection>`, which the connection's
module links into (`lnk-<connection> link`) and carries from. A linked
device's keys read and send as the account, like a token, so they stay
out of the harness's home, as tokens stay out of its settings.

#### When it moves

A move carries what the module names as the link (`lnk-<connection>
state`), never a program it installed, which differs between a Mac and
Linux: the machine the agent left deletes its copy, so one device holds
the account. It's decided when the agent is sent, from its harness: one
that speaks the connection itself keeps the link in its own state, which
moves as it always has.

### Webhooks

Where an agent's webhook listens, whose its post is, and how long a
post's id and a rolled secret are kept.

#### Where does an agent's webhook listen?

On 127.0.0.1 alone, in the agent's bridge, which already runs outside
its sandbox beside it and holds its endpoint's key. Anything public in
front of it is a tunnel you open (a relay client), so the
address a stranger can reach is always one you chose, and `add` never
opens one.

#### Its port

The port is chosen once and kept with the agent's settings, so a tunnel
to it keeps working after every restart. The listener names no
connection: each hook says which module checks its posts (`sift`), as on
a machine that packs agents, where the same module answers the same
`sift`.

#### Who hands it over

The bridge, not the module, hands a post to the agent, through the way
in every message from others takes: who it's from is the webhook's own,
so a module can't send a post as the owner's, and its tools are a
stranger's.

#### Why is a webhook's post a stranger's message?

Anyone who holds the secret can post, and a service that signs (GitHub)
passes on what other people wrote. So the post reaches the agent from
`webhook:<name>`, quoted, never as its owner, with only the tools its
specification gives strangers, none unless it says, and only a harness that can
say who a message is from, and keep a conversation's tools, gets a
webhook. No answer goes back to the poster: `202` says it was taken, and
nothing the agent says leaks to whoever sent it.

#### How long is a post's id kept, and a rolled secret?

A post's id is kept 30 days, the longest any sender Link knows keeps its
own (GitHub's, whose posts sign no time), so one keep covers every
scheme. It's kept only once the agent's harness has taken the post, when
it answers the way in, before its turn ends: a sender gives up long
before a turn might, and waiting would hold the post's place for the
whole turn. One the harness refused, or that couldn't reach it (its
harness down), is answered `503` and let go, so the sender's redelivery
is taken rather than refused as a repeat.

#### A rolled secret

A rolled secret's previous one is taken for 24 hours, its expiry kept
beside it, or until `lnk agent webhook rotate <name> --done`: Link can't see when the sender
switched without carrying which secret matched through `sift`, and a day
is enough to paste the new one in.

## What does a mention in a group get?

An answer, there, on every harness and connection: the owner mentioning
their agent in a group, connection or server gets it answering, as in a
direct message, since silence looks broken. Only the owner's mention is
answered; anyone else's, and a message that doesn't mention it, get
nothing. Link sets it in each harness's settings (OpenClaw's and
Hermes's group policies), and Link Harness's bridges carry the mention,
each group a conversation of its own (`<connection>:<group id>`), so
switching harness doesn't change it.
