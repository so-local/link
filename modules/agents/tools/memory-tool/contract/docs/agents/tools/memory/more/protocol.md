# Interface

The commands a memory engine, `lnk-<engine>`, answers for an agent,
typed in `link-memory-tool-contract`.

## Declared commands

The protocol is declared once, in the contract's own
[`actions.toml`](https://github.com/so-local/link/blob/main/modules/agents/tools/memory-tool/contract/actions.toml)
and [`types.toml`](https://github.com/so-local/link/blob/main/modules/agents/tools/memory-tool/contract/types.toml):
`install`, `launch show` and `serve-many` with their arguments, and
`launch` and `agents`, what `launch show` prints and what `serve-many`
reads. Its
build generates them into `link_memory_tool_contract::declared`, each
command's arguments with an `argv()` that writes its line
(`LaunchShowArgs`, `MemoryInstallArgs` and `MemoryServeManyArgs`).

### Lines from one declaration

`lnk-agent` builds every line it runs from those arguments, and an
engine lists the commands under `[implements]` in its own
`actions.toml`, so its parser is generated from the same declaration.
Link Memory's says `memory = ["memory.memory.install",
"memory.launch.show", "memory.memory.serve-many"]`. The argument
names, positions and flags are the same on both sides, and the flags
`argv()` writes come in alphabetical order.

## Memory engines (`lnk-<engine>`)

The variables, the session lines and the bindings are typed in
[`memory.rs`](https://github.com/so-local/link/blob/main/modules/agents/tools/memory-tool/contract/src/memory.rs).
[`lnk agent memory start <engine>`](../../../../../../docs/agents/tools/memory/README.md#memory-start) runs `lnk-<module>` (`MEMORIES` in
the agent module maps `link` to `memory-tool`; any other name is its
own module, from Link or not).

### `lnk-<engine> install`

Installs the engine, or leaves it, asking no questions. For a sandboxed
agent, it runs confined with `HOME` set to `<state>`, which it may
write, and no network: the engine came as a signed module, so its
install fetches nothing, and it installs on a cloud's machine too, whose
sandbox is never given the machine's whole network. With the agent’s
sandbox disabled, the adapter runs directly with the same `HOME`.

### `lnk-<engine> launch show <folder> <state> [<agent>]`

Prints a `launch` (`link_memory_tool_contract::declared::Launch`), `{"allow"?,
"argv", "model"?, "models"?, "programs"?, "shared"?}`: the command serving MCP over stdio
for `<folder>` (an agent's Home, or a team's shared folder), with the
agent's conversations and skills in its own folder `<agent>` where it's
given, keeping what it keeps in `<state>`.

## What `launch show` prints

Each key of `launch`, read by `lnk-agent` at every start. A key it doesn't
declare is refused.

### `argv`

The command, its first the engine's own program (`lnk-<engine>`, whose
path its sandbox reads); a command naming another is refused.

### `allow`

The calls that don't ask: those that only read, and one that writes
only the agent's notes.

### `models`

Whether it reaches this machine's model runtimes. With `model`, its
sandbox reaches that model's port alone, through the proxy on Linux; with
no `model`, it has the machine's whole network, which a cloud's machine
refuses.

### `model`

An embedding model to serve for it: `lnk agent memory serve` runs `lnk
model serve --model <model> --json` outside its sandbox with each session, and
gives the engine its URL as `LNK_MEMORY_MODEL`.

### When it isn't served

For one agent (`lnk agent memory serve`), `LNK_MEMORY_MODEL` is empty
when no runtime here has the model or it doesn't say where it listens
within 10 s, so the engine starts well within a harness's wait for its
tool. The service serving an owner's agents (`lnk agent memory
service`) waits for no model: each agent's engine is given the address
its model was started at, and searches by words until the model answers
there.

### `programs`

The Link modules whose programs it runs, by name (Link Memory names
`document-converter`), each read by its sandbox where it's installed and
left out where it isn't. Installing one since the last start defines
the tool again.

### `shared`

With `shared`, the engine serves many agents from one process, and
Link runs it so for every sandboxed agent of one owner on the machine
(Link Memory says it): [One process for many
agents](#one-process-for-many-agents).

## What the engine is given

The variables Link sets for the engine's commands, from the agent's
specification, its sandbox file and its harness's contract.

### `LNK_MEMORY_SPEC`

Every command of the engine's gets the agent's opinions for it: its
specification's `[tool.memory]` as JSON, `model` the embedding model to rank by
(`none`: by words only, no `model` printed), `tokens` the most a search
gives and `notes` the most the agent's notes hold (`"4 KB"`). An engine
refuses one that isn't valid.

### `LNK_MEMORY_NOTES`

Unless the agent's sandbox file locks its instructions (`instructions`
under `[files] read`, not `write`), `launch show` gets the notes file,
`<agent>/memory/notes.md`, which the engine may write and nothing else
of either folder (Link Memory serves it as `remember`). Unset, it
writes none.

### `LNK_MEMORY_FROM`

Where the agent's harness is sent messages from others (its contract's
`from`), `launch show` also gets `LNK_MEMORY_FROM` = `1`: each call then
names its conversation in `_meta` (`link.local/thread`), and the engine
refuses a call that names none.

### Notes only in the owner's conversation

The engine then writes the notes only in a conversation of the
owner's, never a teammate's (`agent-<id>`), a team's (`team-<id>`), a
subagent's (`task-<id>`, which can't prove its parent was the owner's)
or a stranger's (named as its sender).

## How Link runs it

`lnk-agent` defines it as the agent's tool `memory-<agent id>` at every
start, and gives the harness its address as `LNK_MEMORY` and, to its
adapter's `configure`, as `memory`.

### Its sandbox

It's run by [`lnk sandbox run`](../../../../../../../../../security/sandbox/docs/tooling/sandbox/guide.md#sandbox-guide) reading `<folder>`, `<agent>`'s
`chats/` and `skills/`, the program itself and each of
`programs` installed, and writing `<state>` (`<agent>/memory/<engine>`)
and, with `LNK_MEMORY_NOTES`, the notes' folder `<agent>/memory`.

### A store driver's conversations

An agent whose conversations a store driver keeps has the driver's
program read in its memory's sandbox too. Its memory runs with
`LNK_AGENT`, and `LNK_AGENT_ID` and `LNK_OWNER` when the agent has them
(in the `env` of a shared engine's agent, in `agents`), to name the agent to the driver as
its harness does. Link Memory makes its own copy of such a
conversation, and of a compact one, for its search.

## One process for many agents

With `shared`, the tool `memory-<agent id>` is `lnk agent memory
connect <dir> --agent-id <agent id>`, one session on stdio, and `<dir>` is
`~/.lnk/data/agents/memory/<owner>`, which every sandbox hides. It holds
`agents/<agent id>.json`, what each agent is served with.

### Starting the service

A session finds the service's socket in `<dir>` and starts the service
when it isn't running: `lnk agent memory service`, in a `systemd-run
--user` unit of its own where systemd runs Link's services, else in a
session of its own.

### `lnk-<engine> serve-many <socket> <agents>`

The service serves each model once outside its sandbox and runs
`lnk-<engine> serve-many <socket> <agents>` by `lnk sandbox run`. `<agents>`
is a file holding `agents` (`link_memory_tool_contract::declared::Agents`) as
JSON: each agent's ID to an `agent`, `{"argv", "env"?}`, the command `launch show` printed for it and its
`LNK_MEMORY_MODEL`. The file names its schema, `"schema": 1`, which an
engine reads through `link_files::saved::parse_json`; a file with none is
read as the current one.

### The service's sandbox

It reads every agent's `<folder>` and its `<agent>`'s conversations and
skills, the program and the folder each agent's storage bridge's memory
socket is in (`<runtime>/lnk/agent/memory/<owner digest>/<agent id>/`,
in the runtime folder), and writes every `<state>`,
every notes folder and `<dir>`.

### Its socket

Each connection to `<socket>`, a Unix socket, is taken only from a
process of the user the engine runs as, or of `--peer`'s, the socket
then shared with `--socket-group`’s members, mode 660, in a folder no one else
enters.

### Its flags

The engine removes its socket and ends once no session has been open
for a moment, unless run with `--stay`. `--cache <folder>` keeps the
index of a bound agent `<agents>` doesn't name, in
`<folder>/<owner>/<agent id>/`, and `--embed <model> --embed-url <url>`
ranks its searches by meaning; `<agents>` may then be left out.

### Its agents changing

A change to the agents served names a new socket, and the old service
ends after its last session.

## A session's first line

Each connection starts with a line written by Link, answered `{"ok":
true}` or `{"error"}` (`Welcome`, then closed), then MCP as over stdio.

### `Hello`

`{"agent": "<agent id>"}`: the session is answered from that agent's
`argv` alone.

### `Bound`

The same with `"bound": {"owner", "service"?, "key", "files"?, "storage"?,
"notes"?, "index"?}`, where its machine keeps what memory reads off its
folder: its key for this start, which proves each call to its bridges,
the files bridge's socket where its Home is in a bucket, its places and
the storage bridge's socket (`link_store_contract::storage::StorageAt`),
and the most its notes hold where it writes them. `service` identifies
the service principal the agent acts as when its bridge requires one.

### `index`

Where the engine keeps its index copy: `memory` by default (the
resolved memory place, falling back to Home), or `home`, which requires
the files bridge.

### What it reads and writes, bound

Bound, its conversations, notes (`memory/notes.md`) and index's copy
are read and written through the storage bridge wherever its machine
keeps each (the copy as files of its `memory` collection,
`memory/<engine>/`, `link_store_contract::store::memory_files`), and
its `memory/` through the files bridge.

### Its index, bound

Its index is a cache of the copy, kept only while it's of the copy
there now, written back at most once a minute while it changes.

## Team Sessions

A team session uses the member’s binding while selecting the team’s
shared folder and index policy.

### A team's memory, bound

The same with `"team": {"id", "named", "writes"?, "semantic"?}` beside `bound`: the
team's ID, the team as the agent names it to its files bridge (`<name>`
its own account's, `<account>/<name>` another's), and whether the agent
writes the index and whether its policy enables semantic queries.
The session is the team's memory, its own of
that agent's, by owner, agent and the team's ID.

### What it reaches

It reads the team's folder's `memory/` through the files bridge, as the
agent, and keeps its index's copy beside that folder, in the folder's
storage, where the bridge serves it only to a member granted all of the
folder: a member granted part of it is refused at its first call. It
writes no notes and reads no conversations. A `changed` control naming
the team lists that folder again; the agent's `flush` and `end` write
back and end its teams' sessions with its own.

### Who keeps the index

Only the eligible index keeper (`writes`) writes the index back. Other
members read the copy and search by words until `semantic` is enabled.
Then their queries and newer changes use the team's paid embeddings.
The endpoint checks current membership, whole-folder access, opt-in,
payer consent and the shared daily allowance before paid work. A session
cannot choose its own payer or allowance.

## Machine Bindings and Controls

The service accepts a binding only through the machine’s own bridges.
Its controls then refresh or end the associated retained sessions.

### The machine's own bridges

The bridges `Bound` names must be the machine's own, as the service is
told (`--files-bridge <socket>`, `--storage-bridge <socket>`, or the
agent's own `<folder>/<agent id>/storage.sock` in `--bridges
<folder>`): any other is refused before the key goes anywhere, and the
agent is served only once one of them takes the key.

### `lnk agent memory connect`

It binds an agent so when its storage bridge's plan keeps its
conversations, instructions or memory off its folder, with the key this
machine's keyring holds for its start, and refuses while the agent
isn't running.

### The memory socket

`connect` reaches the bridge at the memory socket (`lnk-agent`'s
`bridge::memory_storage`), which changes, besides what the agent's
sandbox writes, only its `memory` collection and, where its notes are
written, `memory/notes.md`.

### `Control`

A line may instead be a control, named by owner and ID: `{"changed":
{"owner", "agent", "path", "team"?}}` (a team's, with its ID as `team`),
`{"flush": {"owner", "agent"}}` (write its own index back now, then
its teams') or
`{"end": {"owner", "agent"}}` (drop its key).

## At an embedding endpoint

A bound engine proves itself at a Unix embedding endpoint with
`Authorization: Link-Binding <agent>:<proof>`
(`link_memory_tool_contract::embedding::Authentication`), a credential for
that binding and purpose alone.

### The proof

The proof is 64 hex characters, made with the binding's key over `memory
embedding binding` and the binding's identity. For a team session, the
header appends `:<team ID>:<files-bridge name>`, and those values are
also part of the proof’s challenge. A header naming an
invalid agent, or a proof of any other shape, is refused before it's
checked.

### Who it's for

The endpoint resolves the agent against the machine's live binding,
and the proof matches only that binding's agent and key: no owner is
asserted by the engine.
