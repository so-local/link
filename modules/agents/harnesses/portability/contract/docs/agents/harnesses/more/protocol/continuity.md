# Portable Context

An adapter can capture readable conversation records and read shared
memory through Link's authenticated storage bridge, whichever carries
its connections. Its native sessions, context policy and resume
mappings remain its own.

## Storage binding

A harness keeping Link's collections receives `Settings.storage` when
its restricted folder layout supports the bridge. Its native sessions
and the state exchanged over the Agent State Protocol (ASP) remain the harness's own.

### The binding

Link hands `configure` the resolved storage and this start's binding
secret in `endpoint_key`. Keep that secret in private configuration,
never `Launch.env` or a command line. Use the supplied agent ID and
owner to build a `Binding`.

## Read and write collections

Use the existing `store::Request/Reply` through `Bridged`. Link selects
the driver for each request's collection, installing the driver when
needed.

### The `file` place

For the `file` place it runs `lnk-file-store` rooted at the protected
agent folder, with the agent's ID, outside the sandbox. It gives the
harness no unrestricted mount of `agent/`.

### What the bridge checks

The bridge checks the proven agent and owner against the request,
validates collection paths and uses the existing no-follow file driver.
File versions are opaque `u64` CAS tokens: compare them for equality
only. Conversation sequence conflicts, leases and file CAS keep their
existing meanings.

### Grants

Instruction and skill writes follow the agent's grants. A writable
Home, or its `memory/` documents, grants no write to the protected
memory index. The existing notes grant also permits portable documents
under `memory/portable/`. Memory's separate socket retains its index
write scope. Source metadata never expands grants.

### Notes

Notes at `memory/notes.md` retain the agent specification's size limit and
require versioned `FileWrite`, so an unbounded append cannot bypass
that limit.

## Capture exposed records

`store::context::Event` is an owned view of the common fields, with
`seq`, typed `role`, `text`, `at`, optional `from` and optional
`source`.

### Roles and defaults

Roles are `user`, `assistant`, `tool` and `summary`. Missing optional
text/time fields default to empty strings, while `from` and `source`
default to none. Unsupported native record types can remain in the raw
log without becoming a portable role.

### Source correlation

New native captures include stable source correlation, with `adapter`,
native `session` and native `record`. Correlation is scoped to the Link
agent and thread. Neither a transcript role nor a source label
establishes who may act.

### The archive

Preserve original JSON and unknown extensions. Read the raw log as the
archive, and avoid writing the smaller event view over it. The
owner-facing chat copy omits tools, summaries and unfinished turns, so
it is unsuitable as an archive. Unexposed reasoning does not become
readable context.

### `context::append`

`context::append` assigns the next existing sequence and detects a
repeated source. After an uncertain append response, retry the same
immutable record with the same source: it finds the original sequence.
Reusing a source with different content is an error. The caller
persists native record IDs before trying a capture and retains
uncertain work for retry.

### The capture queue

The optional native-hook client syncs each admitted capture batch into
`.lnk/data/agents/context/captures/` before contacting storage. It replays pending
batches in order before another capture or context preparation, using
the same source correlation after a lost acknowledgement. Completed
batches are removed only after every record is confirmed. A private
lock serializes native-hook clients.

### The queue's limits

The queue holds at most 1,024 batches and 64 MiB, with at most 16 MiB
per batch. A full queue drains when storage recovers. Corrupt,
incomplete, foreign or unsupported queue entries retain their originals
and refuse context preparation.

### What the queue covers

It covers records admitted by the client, not uncaptured native history
or a host switch/export barrier. Native hooks can report a failure and
continue.

## Keep versioned memory

Portable documents are JSON files at `memory/portable/<id>.json`, using
existing `FileRead/FileWrite`.

### `context::Memory`

`context::Memory` holds `id`, `text`, optional `source`, `evidence` as
Link thread/sequence references, and `modified_at`. Unknown metadata is
preserved. The store's opaque version is the revision, separate from
the document. It is neither a timestamp nor an ordered counter.

### Deletion

An optional `deleted` flag defaults to false. A deletion keeps the
document ID, source and evidence at a new CAS version, with empty text
and `deleted: true`. It stays visible to context consumers as a sourced
deletion. Removing the portable JSON file itself remains a separate
storage operation.

### Native Memory Revisions

Personal adapters also carry ASP notes by their original source and path.
A complete export retains sourced tombstones for disappeared native files.
On return, a correction or deletion updates the actual native memory file.
An intervening native edit conflicts with the last exported revision and
refuses continuation until reconciled. Foreign-memory blocks are rebuilt
from current revisions, so an older copy cannot restore a deleted note.

### Link Notes

The current Link notes document supersedes older sourced copies in retained
snapshots. A missing or empty document supplies an explicit deletion, so
removing Link notes also clears generated native memory and next-turn
context instead of reviving an older recollection.

### Native exports against a deletion

Old native exports cannot overwrite a deletion when their last
successful export is unchanged. A changed native file conflicts with an
intervening shared deletion until reconciled.

### Each adapter's exports

The optional native-hook client keeps each adapter's exports under its
own IDs. It retains native originals, records the last successful
export's digest and version in the adapter's state, preserves shared
corrections when the native file is unchanged, and refuses divergent
edits. It supplies other adapters' documents as sourced data without
overwriting a vendor's memory files.

## Read a finite context

`context::read` reads `Last = N`, then the raw log through N and the
requested memory versions.

### `Snapshot` and `Cursor`

Its `Snapshot` retains original events and a `Cursor` holding the Link
thread, `through_seq` and referenced memory-file versions. Version zero
means that a referenced file was missing. `Cursor::current` detects
changed or deleted referenced files and later conversation activity.

### A finite boundary

This is a finite boundary, not a transaction across resources or a
claim of model consumption.

### On return

Keep the native session mapping and last supplied cursor in the
adapter's home. On return, reconcile intervening records and memory
versions before selecting context. A new native session can be seeded
from the same inputs. Never replay tool effects merely because their
records appear in historical context.

## Keep host session and approval metadata

`link_harness_contract::harness::authority` supplies host-only session
and execution-generation types. Its `Ledger` is an explicit local primitive.
Existing adapters, commands and tool proxies do not activate it or
advertise an authority profile.

### Sessions

The host associates a validated `link:<name>` conversation with a
permanent session ID. It reuses `store::Agent` and the current
`bridge::Binding`/`Keyring`, and requires an immutable agent ID. Other
thread formats and pre-ID agents are refused by this primitive; their
existing behavior is unchanged. No history or memory is copied into the
ledger.

### Its fields

The adapter's opaque binding reference, host turn-attempt ID and host
execution generation are distinct fields. Native graph IDs and callback
handles remain adapter data.

### `session-authority.json`

The host chooses a protected directory outside every runtime mount.
`session-authority.json` holds session mappings and one active attempt,
and names its schema (`"schema": 1`), so a newer one is refused, naming
the upgrade.
It holds only a fingerprint of the current start
secret. Agent collection storage cannot read or write this directory.

### How it's kept

The directory is private, its files are private regular files, and
reads refuse links. A directory lock serializes changes. Writes sync
the new file, rename it atomically and sync the directory before
returning. Metadata is limited to 1 MiB; full, corrupt, foreign or
unknown-version metadata is refused.

### Questions and Approvals

The [interaction engine](../../../../../../../../../../packages/interactions/docs/more/libraries/interactions/interactions.md) owns questions, accepted owner responses, reviewed effects and single-use claims. The session ledger has no separate approval records or decision writer. Host binding checks remain necessary at source execution, and an interaction response cannot revive an obsolete native callback.

### `retire` and `recover`

After containing the old execution, the host calls `retire` or the
explicit restart barrier `recover`. The old attempt is retired while its session mapping remains. This primitive offers no native callback
rebind. Retrying admission after a lost acknowledgement returns the
committed attempt and generation.

### Its schema

These types derive JSON Schema from the same Serde definition with the
existing `schema` feature. They describe host metadata, not a native
lifecycle protocol.

### What it fences

The ledger fences its own operations; it neither stops a native process
nor replaces store leases, confinement or the tool proxy's invocation
context.

## Over the Agent Client Protocol

A session's lifecycle, cancels and requests to the owner are the Agent Client Protocol's (ACP's), the
[ACP](https://agentclientprotocol.com)'s, [which
every harness speaks](README.md).
