# Decisions

Why conversation routing, turn handling and terminal chat work this way.

## Messages

Which agent a message is for, how many agents share a machine, what a
message mid-answer does, and how others' messages, subagents and teams
reach an agent.

### Which agent is a request for?

The one its `Link-Agent: <agent>` header names, sent by every bridge on
each message and its proof. A header, like the thread, since chat
completions has no field for it.

#### Answered as that agent

An adapter whose `contract` says `agent` answers a request as the agent
it names, with that agent's key and settings, and one naming none as its
own agent's. It refuses one naming an agent it doesn't run before the
model sees it, so one process could answer for many agents. Link Harness
and the mini harness read it. An adapter that doesn't answers every
request as its own agent's, so the contract stays additive. Each agent
runs a process of its own, [unless the machine shares one](../start/decisions.md#why-is-one-harness-for-an-owners-agents-off-by-default).

#### `Link-Agent-Id`

Beside it, `Link-Agent-Id: <id>`: many owners have an agent of the same
name, such as `onboarding`, so a process
answering for more than one owner's agents needs the ID. It's a header
of its own, not the ID in `Link-Agent`, which keeps the name every
adapter already reads and which an ID could be taken for. Link Harness
keys everything of an agent's (its folders, its settings as last read,
its tools' listings and sessions) by its ID and owner, never its name,
and a test runs one process for two owners' `main`s.

### What does a message sent mid-answer do?

What the agent's specification says (`[turns] busy`), since whether a second
message should change the answer, wait, or stop it is an opinion; Link's
is to steer, as OpenClaw does. Whatever it does, the message is kept at
once, so nothing said is lost to a turn that crashes.

#### One turn a conversation

One turn runs per conversation, whichever process runs it: a lease in
the conversation's folder (`turn.json`), renewed as often as the specification
says (`[turns] renew`; Link's: every 10 seconds), says who. A lease not
renewed for as long as the specification says (`[turns] lapse`; Link's: a
minute), judged to the millisecond, belongs to a process that died: how
soon a crash is noticed against how long a slow machine may go without
renewing is a trade, so both are keys.

#### Where the lapse is read

The store reads the lapse from the agent's settings, not from the lease,
because a crashed process can't say how long its lease lasts.

#### A crashed turn

The next to take the conversation finishes its turn first: its tool
calls that hadn't answered are answered as not made again, and the model
decides whether to call again, so nothing runs twice unseen. A turn that
had answered every message before it crashed only has its lease let go:
asking again would send a second answer.

#### Its answer

An answer finished that way at the harness's start was waited for by
nobody, so the agent's bridge, outside the sandbox, asks the harness for
it (`/_link/recover`) as it starts and when the harness answers again
after it couldn't be reached (it stopped, so it may have crashed), not
on a timer that reads every conversation.

#### Where that answer goes

It sends it where that turn's answer was to go: the bridge keeps, from
each message from others until its answer went, where the answer goes
(nowhere, the chat, or a subagent's parent, up the chain); an owner's
goes to the chat. Asked in two steps, the harness keeps it until the
bridge says it was sent, so one lost on the way is handed over again
rather than dropped.

#### One taker

Taking a lapsed lease is one taker's at a time (`turn.take`), so two
processes never both finish a turn. Every event is written once,
conditionally on its number, so a turn and a message arriving race
safely: the one that loses reads again.

#### A steered message

A message that steered a running turn is answered with a header and no
body (`Link-Steered`, status 202): its answer comes on the request that
started the turn. The bridge marks the message instead, as the specification says
(`[connection.<name>] steered`; Link's: no mark). Link's specification marks no
steered message: the answer on the first message already shows it was
taken in, and a reaction is your choice.

#### Steered as the turn ends

A message steered in as the turn ends is answered by the turn, which
takes the lease again after letting it go when the message came after
its last look, else by the message's own request. So none is left
unanswered.

### How does a message from someone else reach an agent?

A subagent, a schedule, and a message from another agent look like three
features, but they're one: a message for the agent from someone who
isn't its owner, now or later. The hard part isn't writing down when;
it's something holding the message until it's due and waking the agent
then.

#### One tool and one runner

Since all three are a message from someone else, one tool and one runner
do all of it: Link Scheduler, a program of its own in Link's tools, and
the core only gains a header (`Link-From`, with `Link-Hops`) and the
bridge's way in. The bridge already holds the agent's endpoint and key,
restarts with the machine and delivers answers to connections, so the
runner runs from it, through a driver that tests swap for one in
process.

#### Kept as theirs

A message from someone else is kept as theirs, the model is told who's
speaking, and it never counts as the owner's. That every message carries
a hop count and every agent a cap is decided; the numbers are the
agent's specification's.

#### An answer nobody waits for

An answer nobody waits for goes only to the owner's own chat (an allowed
user's on Telegram and Signal, a direct message on Slack and Discord),
or to a group an allowed user talked to the agent in, never another chat
the harness names, since a harness could otherwise make the bot message
anyone.

#### Its groups

The groups are kept by the bridge, in the connection's folder, which the
harness never reaches, and an agent whose specification never answers in groups
sends to none. Signal holds one connection, the bridge's, so a message
sent there goes through the running bridge.





## Link's own connection

Why Link has a connection of its own, and how its answers and Ctrl-C
work.

### Why does Link have a connection of its own?

So you can talk to your agent, and a script can check what it says, with
no bot, phone or third party: every other connection is someone else's
app, made for each agent, and some can't be scripted at all. `link` has
no account and no pairing; its owner is whoever reaches the agent's
endpoint the way its bridges do, which is its user on its machine. Its
threads are `link:<name>`, `link:main` unless named.

### Why does `lnk agent chat send` send as a bridge does?

So a harness speaks it with nothing new: a message is POSTed to its
endpoint with its thread, key and proof, exactly as a connection's
bridge sends one (`link_connection_contract::ask_for`), and the
conformance kit's endpoint checks cover it. A harness that speaks its
own connections has no endpoint to send to, so `lnk agent chat send` refuses
it, saying so, rather than reach it another way. An agent on a box is
reached through [`lnk box run`](../../../../../compute/box/docs/services/boxes/ssh.md#run-lnk-on-a-box), its own `lnk agent chat send` there: nothing
new on the box.

### Where does an answer nobody waited for go?

To the next chat on its thread. One to a message from others in a `link`
thread (a reminder) is held by the agent's bridge, in the connection's
folder, as a connection's module would send it to its chat. One the
harness finished after its caller went (a crash, or Ctrl-C) stays with
the harness, which keeps it in two phases: the bridge leaves a `link`
thread's there, and the chat on that thread asks for it, so one ask
hands it over and nothing is held twice.

### What does Ctrl-C stop?

The chat's wait. A harness runs a turn to its end whether its caller
waits or not, and keeps the answer, so Ctrl-C loses nothing: that answer
comes at the next message. A message sent while that turn still runs
does what the agent's `[turns] busy` says, as on any connection:
by default it joins the turn, and the chat waits for that turn's answer,
which answers both. Which kept answer that is, the harness doesn't say,
so the chat notes what's kept in the thread before it sends and takes
the first answer kept after: one turn runs in a conversation at a time,
so that's the turn it joined.
