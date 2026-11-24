# Decisions

Why a harness program meets Link as it does, where the
[interface](README.md) leaves a choice.

## Switching harnesses

The two harnesses' contracts say which state a switch carries over the Agent State Protocol (ASP)
and which capabilities it leaves behind. Native file formats belong to
each adapter, so the switch asks the contract rather than reading them.

### Why does a switch say what stays behind, rather than carry it?

An export does not imply that another harness can import it. The
contract declares both directions of ASP state separately, so a switch
carries conversations where the source exports them and the destination
imports them. It names the unsupported parts before changing anything;
the source harness's own state stays available for a switch back.

### Why does a switch ask before it leaves something?

A switch you didn't expect to lose something is the one surprise worth
asking about, so [`lnk agent harness set`](../../../../../../../../agent/docs/agents/harnesses/use/README.md#switch-in-one-command) asks before it switches, and `--yes`
agrees ahead for a script.

### Why is what stays behind computed from the two contracts?

Each contract declares `state.export` and `state.import` for its
conversations, `native_memory`, `identity` and `notes` for memory, and
`features` for skills and persona. `Contract::left_behind` computes the
answer from both contracts, tested without a harness running. An outside
adapter gets the same behavior by declaring the same fields; absent
state capabilities promise no conversation transfer.

### Why does a switch carry conversations whole?

ASP represents a session as its Agent Client Protocol (ACP) updates, so an export need not
replace a conversation with a summary. Each adapter's import determines
which updates it retains: the text projection keeps user and assistant
messages and omits other content. A `sessions` capability declares the
supported direction; it does not certify native-state fidelity.

## Why is portable context independent of connection transport?

A native harness can keep its gateways while reading and contributing
the same durable records. The typed protocol uses the existing
authenticated bridge, store protocol, agent identity, leases and file
CAS. The local file driver stays rooted in protected collections, so
access needs neither a chat endpoint nor an unrestricted folder mount.
Automatic agent startup grants storage only to harnesses declaring a
restricted collection layout; their local file collections remain there.

### Portable views

Portable views give common meaning to exposed event fields, sourced
memory documents and finite cursors. Original JSON remains the archive.
Native bindings and resume/context policy stay in adapter state, with no
new session, memory or handoff service. Transcript roles and correlation
establish no authority. [Portable context](continuity.md)
documents the client.

## Why may a namespace hold a call no model is offered?

A harness sometimes calls an agent's tools itself. Link Harness keeps a
quiet conversation's compaction with the scheduler as each turn ends,
and the model must never see or make that call. Since the harness is
handed the scheduler only through its namespaces, the call has to live
in one. It is `cron`'s `compact`, declared with `harness = true`.

### Unlisted, still checked

A harness action is left out of the namespace's `tools/list`, so no
model is offered it, and Link Harness refuses a call to any tool it
didn't list. When the harness calls it, the namespace server checks it
as every other call. The provider's declared schema refuses an argument
it doesn't take, and its approval policy still gates the call. The
scheduler declares `compact` for that reason, rather than accepting an
undeclared name, and never asks for it, since no one is there to ask as
a turn ends.

### One way on every machine

The same declaration builds the namespace on the owner's machine and on
a hosted agent's box, so compaction is kept through `cron` on both. A
separate variable for the scheduler, as before, made the box and the
machine hand different tools.

