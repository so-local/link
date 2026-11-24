# Decisions

Why the [harness hub](../hub.md) maps Link's threads onto a harness's
sessions and carries agent state through the same endpoint.

## Why is the hub a package?

Every adapter needs the same Agent Client Protocol (ACP) transport, thread mapping and sender
handling. Linking that code into each adapter's endpoint shares one
implementation without adding another process to a running agent.

## Why does a session keep its thread's name?

A bridge knows its conversation by its thread, while a harness assigns
its own session IDs. The hub persists the mapping and names exported
sessions by their threads. An imported session becomes that thread's
session, so the next message continues it without another mapping in
the bridge.

## Why does state come from the harness or its store?

A harness advertising Agent State Protocol (ASP) handles its own state. Otherwise its adapter
reads and writes its native store, and the hub can export sessions by
replaying ACP's list and load methods. This keeps private file formats
inside adapters. Unsupported imports are reported as not taken, so an
export capability never promises an import the harness cannot perform.

## Why refuse a failed native resume?

A resume failure can mean a missing file, incompatible runtime or temporarily
unavailable store. Creating an empty replacement would hide that failure and
lose the only saved route back to the original conversation. Keeping the
binding makes recovery explicit. Damaged routing metadata also refuses
startup, including native recovery work that could run before a prompt.

## Why does capture hold exclusive ownership?

Exporting or importing while the harness writes a turn could capture
half a conversation or replace an active session. Checking for idle work
alone leaves a gap for the next prompt. The capture barrier waits for
admitted operations and excludes later ones until native calls, file
operations and routing commits finish. Blocking file work retains the
barrier when its caller stops waiting.

## Why commit restored bindings together?

Restoring two threads can swap their native IDs. Publishing one binding
at a time would either reject a valid swap or leave a partial restore.
Validating the final group and saving it once keeps routing atomic.
The native store owns the transaction for the files it imports.

## Why keep a portable archive beside native sessions?

Native resume and native import are different capabilities. An append-only
portable archive preserves complete updates even when a runtime cannot
reconstruct them. Existing bindings remain usable on return. The shared
selector supplies bounded historical data without replaying tool effects.
Refusing an oversized or uncertain continuation preserves the owner's
evidence instead of silently opening an empty replacement.

## Why can an adapter serialize all sessions?

Some adapters allow several saved sessions while supporting only one
foreground operation at a time. The shared lifecycle gate lets an adapter
select that ownership without changing session IDs or adding another
store. Native background work still requires a separate runtime guarantee.
