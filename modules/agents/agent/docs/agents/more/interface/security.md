# Security

How bridges, native context bindings and namespace actions prove callers and constrain authority.

## Native adapter context

The launcher provisions storage for a harness with a restricted
collection layout; its local file collections stay in that layout.
Portable context's typed protocol can be used by a host supplying an
authenticated binding. An adapter contract has no storage opt-in.
Agent and owner binding, collection grants, CAS and leases retain
their checks. An unrestricted Home grants no write to `agent/` or the
protected memory index.

### The notes grant

The notes grant permits portable documents under `memory/portable/`.
Notes keep the specification's byte limit and use bounded versioned writes.
Memory's own socket retains its separate index scope. Native-hook
bindings stay in private configuration, are regenerated each start and
are unbound at stop. Exported transcript roles, memory and provenance
confer no authority. The raw event log retains unknown extensions, while
context renders exposed common fields only.

### The hook client

The optional hook client keeps exposed captures in private, synced retry
files under the adapter's declared `.lnk/data/agents/context/` state. A lock
serializes clients. It replays pending captures before context
preparation and refuses corrupt, foreign or incomplete batches. The
queue permits at most 1,024 batches and 64 MiB, with 16 MiB per batch.
These are adapter-owned records, not host authority metadata. Portable
memory deletions retain their source at a new version with empty text.
Neither replay nor deletion changes collection grants.

### Where it's defined

[Portable
context](../../../../../harnesses/portability/contract/docs/agents/harnesses/more/protocol/continuity.md)
defines the additive marker, records, memory revisions, cursors and
reference client.

### The ledger

The host-only session ledger described there keeps session mappings and
execution-attempt generations outside runtime mounts and agent-writable
collections. It checks the current start binding before admitting a new
attempt. The interaction engine owns questions, reviewed effects and
single-use execution claims; this ledger stores no separate tool decisions.

#### What it doesn't do

The ledger's caller authenticates owners and contains old processes
before recovery. No existing adapter or tool proxy uses this ledger. It
does not establish session isolation for arbitrary native code or
enforce native process expiry.

## Namespace actions

A namespace route carries the calling agent's binding proof. Its grant
names the agent and exact action; the bridge checks both before reading
context or forwarding a call. Context answers include only selected
public fields or the agent's own notes. No input selects another agent,
program, tool definition or credential. Web, scheduler and tool actions
retain their providers' confinement, approval policies and caps.
Namespace mutations are never broadcast or retried implicitly.

### Owner messages

Owner messages and scheduled answers use the selected connection's
recipient checks; a named service resolves only its configured owner
chats. The host loads credentials only for delivery. Unknown connections,
missing conversations and messages over 16 KiB are refused. CLI messages
wait for the next read. Teammates retain their scheduler caps and approval
policy; selecting a service grants no arbitrary recipients.

### Child processes

The action bridge starts the namespace provider with Link's filtered
environment, which removes credential variables. The provider reads
its registered tool definitions and keeps their explicit environment
and approval gates.

### Native Schedule Handoff

Schedule import stops the managed native source before clearing it.
The adapter checks the exact exported revision and refuses changed or
malformed state. Clearing traverses no filesystem links, archives the
complete source and replaces only the managed copy. The user's original
installation is outside this firing-owner guarantee and keeps its files.

#### Non-owner Authority

Definitions and import progress remain outside the harness's writable
state. Link activates schedules only after the native source is cleared.
The scheduler keeps occurrence identity separate from retry time and
uses the existing authenticated non-owner ingress. A timer cannot become
an owner message merely because it targets an owner's chat.

## Every way out of its sandbox

A harness's sandbox lets it out by two kinds of door, and by nothing
else: its proxy, and its bridges.

- **The [proxy](../../../../../../security/sandbox/proxy/docs/security/sandbox/more/proxy/README.md)**:
  the internet, its model and its tools, as its permissions say, with
  the keys and secrets it never sees added outside.
- Its model, secrets, tools and pages are [its routes](../../../../../../security/sandbox/contract/route/docs/security/sandbox/more/contract/models-and-secrets.md#routes-served-outside), each handed,
  proved as the agent, to the bridge of the module it's for, outside
  every sandbox.
- [A proxy started blank](../../../../../../security/sandbox/proxy/docs/security/sandbox/more/proxy/README.md#a-blank-proxy-linux), on a machine that packs agents, refuses every
  call until it's bound to one agent.
- **Its bridges:** programs outside the sandbox, each holding what the
  sandbox never sees and doing one kind of thing for the agent with it.

### A route's bridges

`lnk-model bridge`, `lnk-secret bridge` and `lnk-sandbox bridge`,
reached only through its proxy. What crosses: a model's
requests and answers (inference only), a service's calls by its secret's
rule, a tool's calls by its rules, a page to open, each call's verdict
before it's answered, kept in the wire's log. It holds nothing between
calls: the key, secrets or tools each call's route is given, from the
bind its proxy holds.

### A connection's bridge

`lnk-<connection> bridge`, in `lnk agent bridge`. What crosses: your
messages in, the agent's answers out; for Signal, an answer nobody
waited for at `send.sock`. It holds the connection's token, or a linked
device's keys, and who may talk to the agent.

### The way in

`way-in.sock`, carrying messages from others: a schedule's, a
subagent's, a background command's ending, a teammate's message, to the
agent's endpoint; the answer to a chat of yours, or back. It holds where
each answer goes, and who may message whom (the teams' files).

### The store bridge

`link-store.sock`, in the harness's own folder, for a legacy adapter
keeping the restricted folder layout. A host-supplied authenticated
binding can also expose portable context through typed storage calls. What crosses: collection records, leases and versioned files
through typed store calls, never a raw query or host path. It holds the
driver's program, and whatever it needs to reach its store.

#### Its limits

The store bridge holds 16 calls, 8 of them one agent's, and runs 4
drivers at most (a driver that serves, kept running between calls), and
refuses a `chats/` that's a link.

### The files bridge

`link_store_contract::files`, where the agent's files folder is kept in
a bucket, on a machine that runs agents only while they answer. What
crosses: Link File's and its memory's calls on the files folder: read,
write and list a path of it, and get or put an object of its memory's
index beside it.

#### What it holds

What reaches the bucket, and the files folder's key: every object is
encrypted, its name too, in rclone's crypt format, so the bucket sees
nothing of it.

#### Never another's

Never another agent's files folder, a hidden path, or one outside it. A
team's folder only where a call names the team and the machine serving
the bridge says, at that very call, that the agent is on it, and then
only the folders of it the team's file grants the agent, written only
from the version read. That grant is the agent's role on the team, a
Policy Specification, and each call is checked with `link-policy`'s `allows`, as
every check is (who may do what).

#### Its writes

A write only in a folder its `[files]` lets it write, only from the
version of the file it names (Write uses the last session version when
known; Replace freshly reads the current text before its conditional write), and refused past the files folder's cap: an agent's
writes are checked against it one at a time, and what the folder holds
is counted again each time the agent is bound.

#### Proving and holding

A caller has 5 seconds to prove who it is, on one of 64 places kept for
proving, 4 of them the same user's: a user's fifth closes its own
oldest, never another user's, so 16 users each holding theirs keep none
of them out (a call turned away busy is made again for about 5 seconds).
Once proved, it holds one of 16 connections at most, 4 of them one
agent's and 8 one owner's agents', and waits at most 30 seconds for all
of its call to arrive, or for its answer to be taken.

### Every call

Every call to a bridge is the same three things: the agent's identity
(its ID and owner, as Link defines them), the action, and a proof:
HMAC-SHA256 with the agent's own key of a random challenge the bridge
sends on that connection and the identity named. Never a process's, a
socket's or the system's word for who's calling. The bridge looks the
key up in the machine's keyring and refuses a call whose agent isn't
bound there, or whose proof isn't that agent's, before it does anything,
and one that hasn't proved who it is within 5 seconds of its challenge:

- **A key proves only its own agent.** A harness, however it's steered,
  acts on a bridge only as itself: the store bridge answers only its
  agent's conversations, the way in takes another agent's message only
  as from that agent, and `send.sock` only its own agent's.
- `send.sock` is handed its agent's key as its bridge starts, and checks
  the keyring too: once the agent stops, or starts again with a new key,
  a bridge still running takes nothing proved with the old one. A proof
  is a signature of one challenge, so it gives the key away to no one,
  and is of no use on another connection.
- **The key is made at each start:** the endpoint's key, which the
  harness already holds and Link keeps outside the sandbox. `lnk agent
  start` binds it in the keyring (`~/.lnk/safe/agents/credentials`, 700,
  which every sandbox hides), and `lnk agent stop` and `delete` unbind
  it: from then on it proves nothing.
- **A machine that runs an agent only while it answers** binds what runs
  it there to the agent for the request: it's given the agent's identity
  and a key made for that binding, and when it stops the key is unbound
  and dropped. Blank, it holds no identity and no key, and no bridge
  takes a call of its; a key kept from a binding that ended proves
  nothing, and the next agent bound there gets a new key.
- A test packs two owners' agents on one machine, each a unit with its
  own blank proxy and sandbox and no virtual machine, and from inside
  each tries the other's folder, sockets, proxy, model provider credential and
  conversations, and the machine's loopback: none is reached, and once a
  unit sleeps, its key proves nothing and its folder is empty.
- **One bridge for many agents** tells them apart by the proof alone,
  the same on Linux, macOS and any machine. Each agent's store bridge
  serves only that agent, whatever another agent could prove.
- The endpoint's own proof, the harness proving itself to a connection's
  bridge, is the same signature the other way, under a label of its own:
  an endpoint, which signs any challenge it's sent, is never asked to
  make a bridge's proof.

Every bridge runs on one server, so each holds to the same limits
whatever it does with a call:

- **Five seconds to prove.** From its challenge, a connection has five
  seconds to say who's calling and prove it, the whole exchange however
  slowly it's sent: one that sends a byte now and then is cut off all
  the same. Who's calling is at most 4 KiB, refused once that much is
  read, and refused unless it names an agent's ID and owner; nothing
  of the call after it is read until it's proved.
- **Unproven connections apart, by user.** Connections that haven't
  proved who's calling hold places of their own, never a proved call's:
  4 of each peer's, the user at the connection's other end as the system
  names it, with room kept for 16 peers. A peer's fifth closes that
  peer's own oldest, answered busy, never another peer's; a seventeenth
  peer is answered busy until one goes.
- With those limits, a flood keeps out neither a proved call nor another
  user's proof.
  On a machine where every agent's harness runs as you, they're all one
  peer: one harness flooding a bridge can keep out the proofs of the
  others there while it floods, though never a call already proved.
- **Caps on proved calls:** each bridge holds so many proved calls at
  once, so many of one agent's and so many of one owner's agents' (the
  table above; the way in 64, 16 of one sender's; `send.sock` 8, sent
  one at a time). Past any, a call is answered at once that the bridge
  is busy.
- **One error envelope.** Every call that fails, on every bridge, is
  answered with one line, `{"error": "<why>"}`, with `"why"`
  (`refused`, `busy` or `conflict`) when a caller acts on it, and the
  connection closed. A refusal shows nothing the caller sent.
- A test every bridge passes checks each of these against the bridge
  itself, running: a wrong key, a proof replayed on a new connection, a
  malformed or overlong proof, and an action it can't read, none of
  those run.
- It checks too a caller that never answers or answers a byte at a
  time, an action sent a byte at a time, a peer's flood, one agent's
  calls past its share and everyone's past the bridge's, an answer never
  taken, and the envelope.
