# Harness Hub

> Harness Hub Keeps Conversations Connected

`link-harness-hub` serves a harness that speaks the Agent Client Protocol (ACP) over stdio to
Link's bridges, as Link Harness serves its own: ACP over a WebSocket at
the endpoint's `/acp`, opened with the agent's headers and its endpoint
key. It's a package each adapter's relay is built on, in the adapter's
own `serve`, never a process of its own.

## Native Lifecycle

Launcher resources stay alive until their native ACP child has been stopped
and reaped, even when a connection outlives its hub or initialization fails.
A gateway captured by an adapter's launcher therefore closes after its ACP
client, so native cleanup can release the client's sessions and leases.
On Unix the ACP launcher has an owned process group. Cleanup stops that
group before reaping its leader, including native clients a launcher
respawned. A naturally exited launcher retains its wait ownership until
cleanup, so its remaining descendants cannot outlive the ACP owner.

## The harness behind it

One harness runs behind the hub, started by the adapter's `Start` on
the first message and again whenever its output ends. It answers
`/health` once the harness has agreed on ACP, waiting a little for one
still starting. A permission the harness asks for is given with the
first option that allows, since Link's sandbox is what confines it.

### A harness that can't start

A start that fails, or never agrees on ACP, is tried again only after
a wait: 1 s after the first failure, doubling with each one in a row,
at most a minute. Until then `/health` and every message are refused
with the last failure's reason, so health checks can't restart a
broken harness every second. A harness that answered once and then
stopped starts again at the next message, with no wait.

### A harness that keeps failing

A harness that ended within a minute of agreeing on ACP counts as a
failure too, and one that has run a minute clears the count. After
five in a row (`restarts.toml`) the hub stops starting it: it warns once
on its log, naming the last reason, and refuses `/health` and every
message with that reason until the agent is started again. A harness
that refuses to run past its start, such as one waiting on a migration
of its own, is then never restarted forever.

## Threads and sessions

A bridge names a session by its thread (`telegram:42`), as Link Harness
does; the harness makes sessions of its own. The hub keeps which
session is each thread's in a file, so a thread goes on where it was
after a restart. It uses the harness's `session/resume`, or
`session/load` when that's all it has. Explicit new and resume requests
answer only after opening and saving the native binding, even before
the first prompt. Unaddressed new sessions receive random IDs; repeating
an addressed open keeps the existing binding.

### Operation ownership

A thread's opens, loads and prompts run one at a time. Each session's
updates go back under its thread's name. Adapters using
`Hub::single_session` serialize these foreground
operations across all threads while keeping inactive bindings. Other
adapters can run different threads concurrently. Native background jobs
and children need their own runtime confinement and settlement.

### Cancellation

Cancellation reaches only a session opened in the existing live runtime.
It never starts or reconnects a process to cancel saved work. Native
cancellation remains scoped by the protocol's session ID.

### Failed recovery

A saved session that cannot resume returns an error and keeps its binding.
The hub never substitutes an empty session after refused recovery or when
the runtime supports neither resume nor load. A later compatible runtime
can still take up the original session. A thread with no saved binding
creates a new session.

### Binding integrity

Only a missing binding file means no saved sessions. Unreadable or invalid
metadata refuses health checks and runtime startup until repaired. Empty
native IDs and a native session assigned to several threads are refused.
New bindings become visible in memory only after their atomic file write
succeeds, so a failed write cannot change routing for a later prompt.

## What a prompt carries

A prompt is told taken (`_link/taken`) as it's forwarded, so a
webhook's sender is answered then. When its `_meta.link` says it's from
someone other than the owner, the hub writes who into its text, a line
of its own with their name and hops, each of their lines quoted, so a
harness that reads no `_meta` still knows; a sender that isn't one is
refused before the harness sees it. The `_meta` goes on to the harness
as it came.

### A command by another name

The owner's command goes to the harness by a name it offers, as its
session's latest `available_commands_update` lists them. A prompt that is one
command, an action of Link's namespaces or one of its aliases, such as [`/chat:create`](../../../../../../../link/docs/agents/harnesses/link/commands/README.md#start-a-new-conversation) and its alias `/new`, goes as the first of the action's names the
harness offers; one it offers, or one no
action has, goes as it came. The names are the namespaces' modules'
`actions.toml`'s.

### The first command

A known command waits for its session's command list, including a list
published after `session/new` answers. If no list arrives within the
harness call timeout, the command fails before forwarding. Normal
messages do not wait for command discovery.

## Link's tools

Link's tools and memory go with each session the hub makes, resumes or
loads, as its `mcpServers`: each an MCP server over HTTP at its address
on Link's proxy. An adapter's `configure` keeps them (`Tools::keep`)
and its `serve` hands them to the hub. A harness that says it takes no
MCP server over HTTP (`mcpCapabilities.http`) is given none, and its
adapter writes Link's tools into the harness's own settings instead.

## Agent state

The hub answers the agent state of
[Agent State Protocol (ASP)](../../../../../../agent-state-protocol/docs/agents/harnesses/more/agent-state-protocol.md) for the
harness behind it, each session named by its thread both ways. A
harness that advertises agent state answers it itself. Otherwise the
adapter's `Store` reads and writes the harness's own files, and the
sessions a store doesn't give are replayed through the harness's own
`session/list` and `session/load`, when it has them. Temporary replay
routes restore the preceding destination even when a load fails.

### The capture boundary

Describe, export and import wait up to two minutes for admitted session
operations to finish. Once queued, capture excludes later admissions
through its native calls, file operations and routing commit. A blocking
store retains that exclusion even if its waiting caller is cancelled.
This barrier covers hub foreground operations; adapters still own native
background settlement and the consistency of their own file stores.

### Portable Personal History

Personal adapters opt into an append-only archive under `.lnk/data/agents/context/`.
Admitted prompts and native updates are synced before forwarding or
acknowledgement. Imported updates remain exact, separately from native model
input. Unmapped native histories receive stable archival thread IDs that
never guess a channel, account, group or topic destination.

### Continuation Without Native Import

When native import is unavailable, the hub retains the portable session and
supplies bounded historical context with its next prompt. Existing native
bindings remain available. The context contains current sourced memory,
including deletions, all user text, and recent replies and tool evidence
that fit. Required material that exceeds the budget refuses continuation.
Historical tools are data and are never executed by import.

### Capture Recovery

Each capture record is written privately, synced, renamed and its directory
synced before admission succeeds. Reopening replays the records in order.
Unknown, incomplete or foreign records refuse continuation and retain their
originals. Repeated imports of an already retained prefix do not append it
again. A repeated transport delivery requires outcome reconciliation before
the hub will forward it again.

### Identity and Interrupted Writes

Records bind to the immutable agent, owner and adapter. A complete pending
record is committed once on reopening. A partial record refuses startup.
Imports validate every routing identity and native source correlation before
committing the group. Reusing a native record ID with changed content is an
error. Archive files are limited to 48 MiB each, with oversized capture
refused before forwarding.

### Context Budget

The shared selector reserves at most one quarter of the model's window,
counted conservatively as UTF-8 bytes, capped at 32 KiB. An unknown window
uses 8,192 as its input limit. The supplied record keeps its thread, archive
position and selected and omitted indices. Native context limits still
apply to instructions, cached history and tools together.

### What comes in

An import's sessions each become a thread: under its old name when
that's a valid thread name, else a new one. All returned bindings are
validated and saved together, including swaps between existing threads;
invalid or unwritable routing publishes none of the group. Successful
restore clears the loaded-session and command caches, so the next open
takes up the restored native session. A harness with neither state source
answers that it takes none, kind by kind.

## Its endpoint

The hub refuses a connection without the key, or naming an agent that
isn't its own, and with no key set it serves none (503), as Link
Harness does, and proves it holds the key before it's sent it, as the
[contract](../../../../../../contract/docs/agents/harnesses/more/protocol/contract.md)
asks; `Hub::serve` sets `TCP_NODELAY` on each connection it takes.

## Why it works this way

The hub's [decisions](hub/decisions.md) explain its thread mapping, native
state boundary and why it shares code inside each adapter's endpoint.

## Link-owned namespace commands

Kept tool delivery also holds the agent's proved action endpoints and
names. The hub runs these owner commands through their routes before
forwarding a prompt to the native harness, and returns the typed answer through ACP.
It adds them to the harness's advertised command list. A message from
another sender cannot invoke them. Per-harness conversation commands
continue to use the native session's advertised names and aliases.
