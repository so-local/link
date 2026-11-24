# Decisions

Why agents keep stable identities, separate folders and predictable startup settings.

## Agents

How an agent is named, owned, given its ports and its folder, and which
model it starts with.

### Which process runs an agent?

Link Harness serves its agent endpoint and Agent Client Protocol (ACP) sessions in `lnk-harness`.
The Hermes and OpenClaw adapters serve that endpoint through the Hub library
inside their own process, managing the upstream ACP child. The Hub adds no
separate daemon. Connections send messages to the proved endpoint directly.

#### Foreground Management

`lnk-agent` manages settings, provisioning and lifetime. With
`--foreground`, it remains as a supervisor for signals, readiness, terminal
and log output, cleanup and optional connection jobs. Messages do not pass
through it. A background start gives the launch job to the service manager
and the management command exits. Memory comparisons must name which start
mode they measured and include its retained processes.

#### Why does the foreground owner catch Ctrl-C?

The owner keeps the harness's log pipes open while the harness stops. It
catches Ctrl-C and sends its child a managed stop, including an interrupt
received before that child starts. Exiting immediately closes the readers
too early: a native gateway can spin on broken-pipe errors until its stop
deadline. The owner instead drains output, waits and reaps the child.

### How are agents and machines named?

Each has a name you type and a random ID, kept in `machine.toml` and the
agent's settings. An agent's ID is 12 random characters that never
change. Names are what people say; IDs keep two machines' or two users'
`main` apart, with no registry to hand out numbers. The first `lnk agent
start` makes `main`: most people have one agent and never name it. No
agent or box is named `here`, `home` or `all`, Link's own words where a
command takes a name, so `lnk agent move here` always means this
machine; a computer named one of them before keeps its name.

### What is named after the agent?

What people see by its name: its folder, its settings file, its log and
its rules files.

#### Keyed by its ID

What runs is keyed by its ID, which no other owner's agent shares, so
two owners' `main` never meet even on one machine: its service
(`link.agent.<id>`, `lnk-agent-<id>.service`), the service carrying its
connections (`link.bridge.<id>`, `lnk-bridge-<id>.service`, unless one
bridge carries every agent's), its memory's and its scheduler's tools
(`memory-<id>`, `scheduler-<id>`) and its proxy's sockets
(`<runtime>/lnk/agent/network/<id>`). Nothing of one agent is shared with
another. A harness can't claim any of Link's services as its own: every
name starting `link.` or `lnk-` is refused.

#### Where the ID is read

The ID for these names is read from the agent's settings file alone, as
it is: a service's name is asked for at any moment, and loading an
agent's settings moves old sections and writes the file. An agent from
before IDs is keyed by its name.

### Where does the truth about an agent live?

On the machine running it, and nowhere else. Every Link is equal, so a
list of agents is asked for and never kept, and losing one machine loses
only what ran there. A computer that moved an agent to a box keeps only
a note, so the machine holding the state is the only one that makes its
ID, and migrating both ends would never make two agents with one bot.

### Who owns an agent?

Two kinds of owner, each whole, so everything works without signing in.
Someone who never signed in owns their agents as the user running Link
on the machine, `<user>@<machine>`, from the machine's own properties,
with nothing to set up. The machine's part is its name in
`machine.toml`, made once from its host name and kept, so renaming the
host changes no agent's owner.

#### One owner per agent

Most harnesses assume one person; Link keys each agent by its ID and its
owner's, so two people's agents on one machine, or one process, never
meet. The owner is in the agent's settings, never taken from a request,
and moves with it.

#### Where it may move

An agent nobody signed in for moves to any machine, signed in or not,
and on one signed in it becomes that account's at its next start there;
an account's moves only to a machine signed in as that account, so an
account's agents never land where anyone else, or nobody in particular,
is signed in. One owner per agent; people who share agents are an owner
of their own.

### How does each agent get its ports?

Through `LNK_PORT` on every adapter command. Every agent, `main` too, gets
a block of ports from 20000, or from the machine's `ports_from` [setting](../more/settings/README.md#agents-ports), as many as its harness says it takes (its
contract's `ports`) and one more, for its proxy on a Mac: two for Link
Harness. An adapter that doesn't say gets 200, as every agent did
before. Switching harness grows or shrinks the block at its next start,
at the same first port when no other agent's block is in the way.

#### `main`'s ports

`main` gets a block of its own, as every agent does, never the adapter's
default, such as Link Harness's 18800, OpenClaw's 18789 or Hermes's 8642.
Every user of a machine runs the same adapters, and every user shares the
ports on the loopback address. A default port works for one
user and fails as soon as a second user starts an agent beside them,
such as one [`lnk box user add`](../../../../../compute/box/docs/services/boxes/user/README.md#add-a-user) made. The adapter's default is only for running
it outside Link. An adapter that doesn't take `LNK_PORT` is refused,
since its ports would be another agent's.

#### A free block

A block is free of the other agents of the user running Link, and of
every port a program on the machine holds when it's given. A start makes
it free of everything else on the machine. Each user's agents allocate
from 20000, so where another program already listens on one of its
ports, another user's agent say, the start moves the agent before it
starts anything, and says so.

#### Where it goes

A new agent's block, one for an agent arriving from another machine and
one it moves to are each a free block picked at random below 32768,
where the system gives programs ports for their outgoing connections at
any moment. Any port there may be a block's first, so a new agent's
block, made 200 long and shrunk to its harness's at its first start,
still lands anywhere. The first free block would be the same for two
users making agents at once, so neither would see the other's in time.
Picked at random, two land on the same block only rarely, and the next
start of the one that lost moves it.

#### A program on its ports

A program on the ports would otherwise be taken for part of the agent,
or be handed its messages; one that takes a port in the moment before
the harness listens leaves the agent unanswered, never answered by that
program, since a bridge sends nothing to an endpoint that can't prove it
holds the agent's key.

### How are agents kept apart?

The sandbox hides `~/Link/Agents` from every harness except its own
agent's folders, and `~/Link/Archive`, what agents that left kept, even
with `--files.read '~'`, since agents run as one user. Their ports stay
reachable wherever localhost is; each harness's own token guards them.

### Its folder

Why an agent has two folders and how they're named.

#### Why is an agent's folder two, Home and Agent?

What's yours and what's the agent's are kept apart: `home/` holds your
documents and its work, `agent/` its brain, each collection a folder
(`instructions/`, `skills/`, `memory/`, `chats/`, `schedule/`).
A Home you pick (`--run.files`, a repository) no longer gets the agent's
conversations and instructions written into it, `agent/` stays put
wherever Home is, and each collection kept at `file` is one folder, so
nothing has two ways in. `agent/` is open to you: you edit its
`instructions/` and `skills/` as plain files.

##### Seen as one

The agent sees both as one, as before: its tools and its sandbox file's
`[files]` name `instructions/`, `skills/` and `chats/` beside
Home's own folders, and a move carries them so (`files/...`). One view
kept every grant written before meaning what it meant, every move
between Links reading the same stream, and every prompt saying the same
paths. `.` is all of Home, no longer the agent's instructions: a
repository it works in is Home, and its own folder is named beside it
where it may write it.

#### How does a restore know a backup carries the agent's own folder?

Its buckets say so: each backup labels them `lnk-layout=2` through your
cloud accounts (`lnk storage label`), and a restore reads the label
before what the backup folder holds.

##### Why not the backup folder

The backup folder sits in Home, which the agent writes, so nothing in it
can say how it's laid out: a version written into the harness's archive
could be removed as easily as `Agent.tar` itself, and so would say no
more than whether `Agent.tar` is there.

##### Why the label

The label is set with your accounts, which the agent never holds, and
the key a box gets for an agent's bucket reaches its objects alone,
never its labels.

#### Why are the agent's folders' names lowercase?

What the agent reads and writes (`instructions/`, `skills/`,
`chats/`, `work/`) is named for it, in lowercase, and side by
side, never one inside another: a sandbox has no read-only folder inside
a writable one, so `instructions/` can be locked (`[files] read`) while
`work/` stays writable. The two folders themselves, `home` and `agent`,
are lowercase too: only the folders directly in `~/Link` and
`~/Link/Safe`, such as `~/Link/Agents`, are Title Case. `chats/` is never writable
to a tool: its harness alone writes it, so nothing rewrites what was
said.

#### Why does `[files]` hold only for some harnesses?

A harness's sandbox can make Home read only and write only some of its
folders only if the harness writes nowhere else there. Link Harness
writes only `chats/` of the agent's own folder, and says so in
its contract (`folders`); a harness from outside Link works in the whole
of Home, so its sandbox writes all of it and never reaches the agent's
own folder, and a start says your `[files]` holds only for Link's tools
there.




### Commands

Why the agent is a flag, and a harness's commands a word of their own.

#### Why is the agent `--agent`, even for `move`?

[A positional argument](../../../../../../docs/more/developing/modules/specs.md#name-a-command) names what the verb acts on.
`move` acts on a destination, as in `lnk agent move here --agent work`.
The agent is selected with `--agent`, which can be omitted when there is
only one. `create`, `remove` and `restore` take an agent as their
object.

#### Why are a harness's commands `lnk agent harness`?

[A slot](../../../../../../docs/more/developing/modules/specs.md#name-a-command) holds Link's words or names you chose, never either.
`lnk agent
<harness> allow` put a harness's name where Link's commands go, so no
harness could be named `start` or `use`, and a mistyped command read as
a harness.

##### The agent's harness

`lnk agent harness-permission add --network.mode host` acts on the agent's
harness, the one you most often mean, and `--harness hermes` names
another, as `--agent` names another agent. `remove` takes the harness as
its object, since it ends it.

## What does removing an agent keep?

`lnk agent delete` takes everything Link keeps for the agent on this
machine, as a move away does, but leaves no note of where it went. Its
buckets stay, since they hold the person's files and removing an agent
is often a mistake found a day later; [`lnk storage remove`](../../../../../storage/storage/docs/services/storage/guide.md#remove-from-your-clouds) deletes them.

### Its files

Its files are the person's, so they go only when the person says so:
kept, they're put aside where a moved agent's are, for as long, and a
files folder they chose stays where it is. Without a terminal, `--yes`
removes the agent and keeps its files unless `--delete-files` says otherwise.

## Many agents on one machine

The trades a person makes to pack a machine with agents: one harness,
one bridge, and an agent that sleeps.

### Why is one harness for an owner's agents off by default?

Because it trades a sandbox for code. An idle agent's harness is six
processes, its sandbox and the harness in it, of the eight it holds; one
harness for all of an owner's agents takes that down to the agent's own
proxy.

#### What it trades

One harness for an owner's agents is one process in one sandbox holding
every one of those agents' folders, and which agent a request touches is
kept apart by Link Harness and its tests, where a sandbox each kept them
apart by the kernel. Each agent's proxy, key and grants stay its own
either way.

#### Whose trade

Giving up a sandbox each is a trade a person makes for their machine, a
box packed with agents, say, so it's `~/.lnk/safe/agents/policy.toml`'s `shared_harness`,
which no agent's sandbox reaches, and never across two owners. An agent
that reaches anything but through its proxy keeps a sandbox of its own,
since one network for all couldn't keep its endpoints its own.

### Why does no process serve two owners' agents?

Because one owner's agents together are one person's, and two owners'
aren't. Memory's service and the harness an owner's agents share each
hold every one of those agents' folders, so each is one owner's: nothing
asks for one across owners, and offering it would be a second trade on
top of the first.

### Why is one bridge for every agent off by default?

Because it trades a fault kept to one agent for fewer processes. Each
agent's bridge is a service of its own, with a scheduler runner beside
it: two processes an agent, idle or not. One bridge for every agent on
the machine, `~/.lnk/safe/agents/policy.toml`'s `shared_bridge`, carries them all from one
process, with one runner a time zone, since a runner's times are its
process's zone.

#### What it doesn't change

Nothing is less isolated than before: every agent's bridge already runs
as you, outside every sandbox, and reads every agent's settings, and
each agent still has its own way in, key and connections' bridges.

#### Whose switch

With one bridge for every agent, a fault in it stops every agent's
connections until it's back, where a bridge each stopped one. That's a
trade a person makes for their machine, so it's a switch they set, which
no agent's sandbox reaches. An idle agent then holds no process of the
bridge's, which is what lets one with no turn running hold none at all.

### Why one scheduler runner a time zone?

Because a runner's times are its process's time zone, which is set once
as it starts, so one process can't hold two. The bridge every agent
shares runs one for each zone its agents are in, and hands it the agents
as a file it reads again when it changes. An agent taken off it stops
only once its firings have ended, so a change of agents never restarts
the runner, and no job is fired twice.

### Why does the bridge listen on an idle agent's port?

Because something must hear a message to wake an agent that holds
nothing. Connections and schedules reach an agent at its endpoint's
port, so where one bridge carries every agent, the bridge listens there.
On Linux behind its proxy it does so always, in the proxy's place,
passing each connection into the sandbox through the proxy's folder, and
on every byte it knows the agent is busy, whatever its harness.

#### Where the harness listens itself

Where the harness listens on the loopback itself, as on a Mac, the
bridge can't hold the port while the agent runs, so it holds it only
while the agent sleeps and hands it over at the first connection, Link's
connections trying a refused connection again for the moment between.
Awake, the agent is busy while anything is connected to its port. Only
bytes pass either way, and the endpoint proves itself with the agent's
key as before.

#### When it sleeps

It stops once nothing has come for `[run] idle_after`, no answer is
still coming through its door, no turn runs and no background command
holds its lock.

#### What counts as an answer

The door reads how each HTTP message on a connection is framed (its
head, then a body by length, in chunks or until the connection ends),
never what it says, so an answer that pauses longer than `idle_after` (a
long tool call, a harness closing its tools) is never cut; what it can't
frame (a tunnel, an upgrade to another protocol) counts as an answer
coming until the connection ends. A connection its client keeps open,
idle, between answers isn't. No ceiling cuts one: an answer that never
ends, like a turn that never ends, keeps the agent awake.

#### How long it waits

How long an idle agent waits before it sleeps is an opinion, memory
against a start from cold, so a key of the specification, and Link's is five
minutes: a person's back-and-forth never waits on a start, without keeping the agent's processes running through a longer pause.

### How fast does an idle agent answer?

A message to a sleeping agent starts its sandbox and harness. Where its
owner's agents share a harness, only its proxy needs starting, and the
sleeping agent remains registered with the shared harness. With Link
Memory, waking the first agent for an owner also starts that owner's
memory service. Startup time depends on the machine, harness and model;
a local model server must already be running.

### What ends a process no agent owns?

Its last agent leaving. Memory's service ends 5 seconds after its last
session, and the harness an owner's agents share and the bridge every
agent shares 20 seconds after their last agent, so an agent started
again meanwhile isn't left with none. Each is a service of its own where
Link's services run, so stopping the agent that started it doesn't stop
it, and in a session of its own elsewhere, which an agent in a terminal
starts again when it's gone.

### What happens to a memory session that arrives as its service ends?

It's closed unanswered, and tried again for up to 20 seconds, reaching
the service still ending or another started once its socket is gone.
