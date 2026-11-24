# Agent Files

Find your agent's saved files and Link's private runtime state, grouped by purpose. [Settings](README.md) explains which files control its behavior.

```sh
lnk agent show
```

## Your Agent's Folders

What you see of an agent, and what its harness works in. A harness
reaches only its own home, its agent's Home, and what its sandbox grants
of its agent's own folder.

### `~/Link/Agents/<name>/home`

The agent's files folder, unless `--run.files` named another.
[`lnk uninstall`](../../../../../../../docs/more/modules/guide.md#uninstall)
keeps it unless you pick `agents`. Link Harness's sandbox reads it and
writes only the folders the agent's sandbox file lets it (`[files]
write`: `work/` by default). A harness from outside Link writes the whole
folder.

### `~/Link/Agents/<name>/agent` (700)

The agent's own folder, each of its collections kept at `file` a folder
of it: `instructions/`, `skills/`, `chats/`, `memory/` (its memory's
index, 700) and `schedule/` (its scheduler's jobs, 700). Link Harness's
sandbox reads its instructions, skills and `memory/` (for its notes),
writes its conversations, and writes its instructions or skills only
where `[files] write` names them. It never writes `memory/` or reaches
`schedule/`, which its memory and scheduler, outside it, write. A
harness from outside Link never reaches this folder.

### `~/.lnk/data/agents/<name>/harnesses/<harness>` (700)

A harness's home, with its install, memory and conversations. On a Mac,
its `.lnk/exec/bin` holds Link's `mktemp`, first on the harness's
`PATH`.

## Services, Logs and Sandbox Rules

What runs the agent in the background, and what records it.

### `~/.lnk/safe/agents/<name>/logs/<harness>.log`

The log of each harness the agent runs, out of every sandbox's reach.
It sits outside the harness's home, so the harness can't swap it for a
link to a file of yours that launchd or systemd would then write into.

### Background Services

One service per agent runs its harness, replaced when it switches
harness: `~/Library/LaunchAgents/link.agent.<id>.plist` on macOS, or the
systemd user unit `lnk-agent-<id>.service` on Linux. A second,
`link.bridge.<id>.plist` or `lnk-bridge-<id>.service`, carries its
connections and stops with the agent. For Link Harness, what links
Signal, `signal-cli` and its account, is kept in
`~/.lnk/safe/agents/<name>/connections/<connection>` (700).

### `~/.lnk/safe/sandboxes/<name>-<harness>*.sb`

The agent's macOS sandbox rules. `<name>-<harness>.routes.json` (600)
beside them names its sealed secrets and holds none, as
[Secrets](../../../../../../security/secret/docs/config/secrets/README.md#nowhere-else-on-the-disk)
says.

## Settings and grants

What you chose for the agent, what you granted it and its harness, and
what a start remembers of the last one. `~/.lnk/safe/agents/<name>/` (700)
holds its log, its foreground lock and its proxy's sockets.

### `~/Link/Safe/Agents/<name>/agent.toml` (600)

An agent's setup and `[behavior]` in [one editable file](README.md#agent-configuration). Commands save
choices here, and no agent sandbox reaches it. This machine's agent is
`main`. A missing file is recreated with default configuration. It
holds only what's the agent's own, so it can be carried to another
machine or person whole: its ID, service, owner, ports and stopped
services are this machine's records, in `identity.toml` and
`here.toml` below, and a file naming one of them is refused.

#### Its Schema

Its first key is `schema`, the settings file's version, which every
save writes, so a later release reads an older file through its steps.
Its `[behavior]` names no schema of its own: it is the agent specification at
the agent specification's current schema, and when that moves the settings file
takes a step of its own.

### `~/Link/Safe/Agents/<name>/sandbox.toml` (600)

[What you grant the agent](README.md#sandbox-permissions), whichever harness runs it. Missing files
are recreated with closed permissions. Permission loading stays closed
independently of whether recreation succeeds.

### This Machine's Records

What this machine keeps of an agent that isn't the agent's own, so
none of it travels in an archive.

#### `~/.lnk/safe/agents/<name>/identity.toml` (600)

Who the agent is on this machine: its ID, `agent:<id>`, the service it
acts as and its owner. The owner says its kind: before you sign in, the
user running Link on this machine, `owner = { kind = "machine-user", user
= "dana", machine = "danas-mbp" }`, and signed in, your account, `owner =
{ kind = "account", id = "<account ID>" }`, as `lnk agent list --json`
prints it too. An agent without one is refused, and any
command's `--agent` takes the ID as well as the name. Deleting
`agent.toml` keeps it, so the recreated agent has the same identity.
It holds no credentials and no sandbox reaches it.

#### `~/.lnk/safe/agents/<name>/here.toml` (600)

What this machine gave the agent: its block of ports (`port`, `block`),
its webhooks' own port, and the services of an install it
was brought in from that it keeps off (`stopped_services`), and the folders of
the `PATH` of whoever last ran `lnk agent start` (`path`), where its run
and code tools find its programs. Each machine gives its own, so none of
it travels with the agent.

#### `~/.lnk/safe/agents/policy.toml` (600)

The machine's own rules for agents: `unsandboxed = false` where no
agent may run without a sandbox, `shared_harness = true` where
[an owner's agents share one harness](../security.md#one-harness-for-an-owners-agents),
`shared_bridge = true` where
[every agent shares one bridge](../../start/security.md#one-bridge-for-every-agent),
and `keep_moved`, how long a moved agent's folders stay. A deployment
may write it before `lnk` first runs.

### `~/.lnk/safe/agents/ids.toml` (600)

Every agent ID this computer reserved or took in, under `live`, and each
one `lnk agent delete` retired, under `retired`. `lnk agent create`
[reserves an ID](../../../../../../../packages/id/docs/more/libraries/types/id.md#reservations) that is neither, and a moved agent arriving with a
retired ID is refused, so an ID never names two agents.

#### Services and Teams

`services.toml` beside it keeps [the services agents act as](../access.md#an-agents-service) alike, each
minted with its agent under an ID of its own, and
`team-ids.toml` teams' IDs, each a group's, `group:<id>`: `lnk agent team
create` and a team's definition reserve one there, a team moved here is
taken in, and `lnk agent team delete` retires it.

### `~/.lnk/safe/agents/harness-grants/<harness>.toml` (600)

[What a harness said it needs](README.md#a-harnesss-grants-harnessesharnesstoml) here and you allowed, for every agent that
runs it.

### `~/Link/Safe/Agents/defaults.toml` (600)

The proxy `upstream` each new agent here starts with (`lnk agent route
--default`).

### `~/.lnk/safe/agents/places/runs-agents.toml` (600)

Whether each cloud account runs agents, as its adapter answered
`agent-runs` once, with the kind and identity it was asked as: `lnk
agent list` asks only those that do, and an account connected again as
another is asked again.

### `~/.lnk/safe/agents/places/cloud-teams.toml` (600)

Where each team a cloud running agents keeps is, by its ID, for a team
an agent here arrived on: its owner, relay and folder service, as the
machine running the move read them. A machine with no account to ask,
such as a box, reaches the team through them. An account here that
lists the team says it instead.

### `~/.lnk/safe/agents/<name>/ports.json`

The harness's ports at its last start, which `lnk agent show` shows
for the agent.

### `<harness home>/.lnk/data/harnesses/configured` and the rest

A hash of the settings, versions and files the last `configure` left, so
a start with nothing changed skips it; delete it to configure again.
What else a harness from outside Link keeps in its home, its
repository's README says.

## Keys, bridges and sockets

How the agent's bridge reaches it and proves who calls, and what a
shared bridge or harness keeps.

### `~/.lnk/safe/agents/<name>/way-in.sock` (600)

The bridge's way in: messages for the agent from its schedules and
subagents, each proved as the agent that hands it.

### `~/.lnk/safe/agents/credentials/<id>.json` (600)

The keyring: each running agent's identity and its key for this start,
which every bridge checks a call's proof against; removed at its stop.

### `<runtime>/lnk/agent/network/<id>/` (700)

`url.secret` (600) is the agent's proxy secret, in its tool and local
model URLs, by the agent's ID. Each start makes it new, before its proxy
and its harness's configuration are made, so an address a program read
in an earlier start is refused. `<harness>` is sockets between the
harness's network and its proxy; `<harness>-<step>` for a setup step's
(`install`, `configure`, ...), and `<harness>-health` for the health
check's relay to the harness's ports.

### `~/.lnk/safe/agents/<name>/bridge.json` (600)

For a harness with an endpoint: its endpoint, key, `proof`, connections
and storage plan. Made at each start and used by the agent bridge to
carry the connections the harness delegates to Link.

### `~/.lnk/safe/agents/<name>/bridge.log`

The log of the connections Link carries, written by the agent's bridge
as it carries them.

### `~/.lnk/safe/agents/<name>/sleep.sock` (600)

Where the bridge tells an agent started in a terminal to stop its
program for being idle, or start it again (`[run] idle_after`).

### `~/.lnk/safe/agents/bridges/` (700)

The bridge every agent here shares where `shared_bridge` is on: whom it
carries (`agents/<id>.json`, written at each start, removed at each
stop), each time zone's scheduler runner's agents (`runners/`), the
agents asleep for being idle (`asleep/<id>`), and `bridges.log`. Each
record and runner's file names its schema (`bridges/record.schema.toml`,
and the scheduler contract's `listed.schema.toml`).

### `~/.lnk/data/agents/shared-harnesses/<owner>/` (700)

The harness an owner's agents share where `shared_harness` is on, and
its HOME: what each agent is served with (`agents/<id>.json`), the
agents' file it's started with, `configure.lock`, held by a start while
it configures and joins, and `shared.log`. Each record and the agents'
file name their schema (`shared/member.schema.toml`, and the harness
contract's `harness/shared_agents.schema.toml`).

### `~/.lnk/safe/agents/<name>/hooks-seen.json` (600)

The id of each post its
[webhooks](../../connections/connect/webhook/README.md)
took, kept 30 days, so a repeat is refused. Written whole by the bridge,
never through a link, naming its schema (`pushed/seen.schema.toml`).

## Storage and context

The agent's storage bridges, and what a harness keeps of the context it
was given.

### `<harness's home>/link-store.sock` (600)

The agent's storage bridge, for a harness that keeps Link's collections
with a restricted folder layout: each call answered by the driver of the
place its collection is kept at here (`lnk-sqlite-store`, `lnk-storage`), run
outside the sandbox. Legacy adapters reach it directly.

### `~/.lnk/safe/agents/<name>/team-files.sock` (600)

The agent's files bridge, for its teams whose folders an account
hosting agents keeps: each call proved with the agent's key, held to its
grants here, and handed to the folder's service with the agent's own
token. Only `file`, outside the sandbox, reaches it.

### `<runtime>/lnk/agent/memory/<owner digest>/<agent>/storage.sock`

The same storage bridge, for the agent's memory: alone in its folder,
which memory's sandbox reads and every agent's sandbox hides, its calls
proved with the agent's key. Besides what the agent's sandbox writes, it
changes only its `memory` collection and, where its notes are written,
`memory/notes.md`. Where it can't be made (its path too long for a
socket, say), the bridge says so and serves the agent without it: memory
then reaches nothing kept off the agent's folder, and the agent's own
store bridge still answers.

#### Socket Path Length

Like every socket of Local Link's, it is in the runtime folder. The
owner's folder is named by the first 16 hex characters of the owner's
SHA-256 (`<owner digest>`), never the owner's whole name, so the path
fits a socket's limit (103 bytes on macOS, 107 on Linux) however long
the owner's name. The owner's memory service is told that folder
(`--bridges`) and takes only the sockets in it.

### `<harness's home>/.lnk/data/agents/context/binding.json` (600)

A host-supplied native-hook binding: its storage socket and adapter
name. A host that supplies one regenerates it for each binding and
excludes it from move state; an adapter has no storage opt-in.

### `<harness's home>/.lnk/data/agents/context/` (700)

Adapter-owned finite cursors, native-memory export digests/versions and
a serialized hook-client lock. Retained across restart and declared by
native adapters as state.

#### `captures/`

It keeps private pending record batches until storage confirms them, at
most 1,024 batches and 64 MiB.

### `<harness's home>/.lnk/data/agents/context/client`

A sandbox-local copy of the adapter executable used by native hooks.
Regenerated by configure.

### `<host-selected protected directory>/session-authority.json` (600)

The explicit host-only session `Ledger`: versioned session mappings and
active attempt/generation, at most 1 MiB. The interaction engine owns
questions and approval decisions separately. Its directory is private (700) and outside runtime mounts. No
command or adapter [creates it by default](../../../../../harnesses/portability/contract/docs/agents/harnesses/more/protocol/continuity.md#keep-host-session-and-approval-metadata).

### `~/.lnk/data/agents/<name>/harnesses/link/config.json` (600)

Link Harness's model and endpoint key, read at each message; written at
each start.

## Memory and tools

What Link's tools keep for an agent, and what its background commands
and answers leave.

### `~/.lnk/data/agents/memory/<owner>/` (700)

The memory service an owner's agents on this machine share. Trusted records
(`agents/<id>.json`), generation caches and `service.log` stay outside its
sandbox. Its HOME and socket are in `run/`, which the engine may write.
The generated `agents-<generation>.json` handoff is a read-only grant, as
is each agent's storage-bridge socket folder in the runtime folder. The engine cannot change its next start's
records or redirect the service log.

#### `generation.json`

The generation those records make, with each record file's inode, time
and size, so a session reads it alone while they're unchanged. It and
each record name their schema (`memory/recording/seen.schema.toml`,
`memory/recording/record.schema.toml`).

#### A bound embedding endpoint

Shared memory's bound embedding endpoint is `unix:<path>` when supplied
as `--embed-url`. Each request [proves the current agent binding](../../../../../../security/docs/config/credentials/agent.md#a-memory-embedding-proof); the
receiver resolves its owner from that proof and its Unix peer. A bound
endpoint over HTTP is refused.

### `~/.lnk/safe/sandboxes/tools/memory-<id>.json` (600)

Its memory, as a tool, started by [`lnk agent memory start`](../../../../../tools/memory-tool/docs/agents/tools/memory/README.md#memory-start), by the agent's ID.
Defined again at a start only when something it's made from changed.

### `~/.lnk/safe/agents/<name>/memory.json`

What the memory tool was last defined from, so a start with nothing
changed leaves it as it is. It names its schema (`"schema": 2`,
`memory/defining/record.schema.toml`) and the final registered command's
fingerprint. A missing fingerprint establishes no managed read authority.

### `~/.lnk/safe/agents/<name>/run.json`, `file.json`, `code.json` and `scheduler.json` (600)

What its `run`, `file`, `code`, browser, desktop and scheduler tools
were last defined from, there while each is defined: the tool's command
line, when its program last changed and the Link that defined it, each
naming its schema (`defined/tool.schema.toml`).

### `~/.lnk/safe/agents/<name>/scheduler-cron.json` (600)

How to take away the calendar entry waking its scheduler, when its specification
says `driver = "cron"`: `{"schema": 1, "remove": [...]}`, the command
`lnk agent stop` runs.

### Its Runtime Folders

What it keeps while it runs that isn't its state, each a folder of
`~/.lnk/data/agents/<name>/`, out of every archive.

#### `~/.lnk/data/agents/<name>/run/`

Its background commands' records (600), written by `run` and each one's
supervisor, swept by its bridge; each kept `[tool.run] background_keep`
once it ended and was told. `started` says when each of the last day's
started (`background_per_day`). They're runtime, not the agent's state,
so they're kept here rather than under `~/Link`.

#### `~/.lnk/data/agents/<name>/caches/`

Its programs' package caches: cargo's (`CARGO_HOME`), npm's, go's,
Gradle's and what keeps to `XDG_CACHE_HOME` (pip's, uv's), each a folder
of it, set by its run tool in each command's sandbox. They're its own,
so a package it downloads never reaches what you build outside it.

#### `~/.lnk/data/agents/<name>/browser/` and `desktop/`

The browser tool's profile, with the agent's sign-ins, and the desktop
tool's display number and cookie (`display`, `auth`). They're runtime
and credentials, never the agent's state, so they're kept here rather
than under `~/Link`. No harness reaches the profile. The cookie is read
by every `run` call's sandbox, beside the display's socket, so the
programs the agent runs can draw on its desktop.

### `<files folder>/background/<id>.out`

A background command's output, both streams as one, its last `[tool.run]
background_output` kept; read by the agent, written only by its
supervisor.

### `~/.lnk/safe/agents/<name>/owed/` (700)

Where an answer to a message from others goes (its chat, another
conversation, or nowhere), kept by the bridge until it went, so an
answer finished after a crash goes there too; a subagent's
conversation's is kept for its own subagents' answers. Each file names
its schema (`bridge/way_in/owed.schema.toml`).

### `~/.lnk/safe/agents/<name>/received/<id>.json` (700, 600)

How many messages each teammate (by its ID) sent the agent today (UTC),
one file each: the bridge takes no more from one a day than the agent's
own `[tool.scheduler] per_day`, and a message the harness never took
counts nothing. Each file names its schema
(`bridge/way_in/received.schema.toml`).

## Teams

What Link keeps for a team, and for its memory.

### `~/Link/Safe/Teams/<team>.toml` (600)

[A team's file](../../../config/specs/team.md): its members, the folder they share, and the opinions they
share, written by
`lnk agent team`.

### `~/.lnk/data/teams/<team>.json` (600)

What the team's memory tool was last defined from, there while it's
defined: while it's there, the team's memory is the tool `team-<team
id>-memory`, and without it the team has none.

### `~/Link/Teams/<team>/` (700)

What Link keeps for a team: its shared folder (`shared`) and its
memory's index (`memory/<engine>`).
