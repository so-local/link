# Control Rules

Link Controls computes input admission and work restrictions from authoritative host snapshots. Callers supply snapshots and persist the returned decisions. The library has no task registry, executor, model connection, or store of its own.

## Select Work

Exact, selected and tree selectors resolve to stable work references with expected execution and control revisions. Tree scope distinguishes Self, Descendants and Subtree. Descendants includes every depth below the selected root. The host supplies authoritative lineage and serializes selection against child creation. The interaction engine persists the resulting members so retries do not expand into a changed tree.

## Compose Restrictions

A pause adds its own reason and advances the control revision. Resume removes only explicitly named reasons. Ordinary pause finishes the current model response while prohibiting follow-on calls. Immediate pause requests interruption. These are desired restrictions until a runtime reports the boundary it reached. Information received while paused can be retained without resuming work.

### Delegation Holds

A hold blocks selected child creation, start or outgoing message actions at the source's dispatch gate. It permits parent thinking and incoming child results. Only the imposing owner may release it. Release leaves other holds and pauses intact. Held proposals still require current recipients, payload, plan and authority checks before dispatch. Pure hold matching is no substitute for serialization at the actual provider.

### Retained Lifecycle

Archive begins scoped shutdown and retains identity, conversation and checkpoints. Unarchive restores paused visibility without clearing independent restrictions. Delete begins removal of its explicitly scoped retained data and requires stopped, reconciled work. None of these operations implies a cascade to children. Hosts report per-member shutdown, uncertainty and unsupported boundaries rather than interpreting acceptance as completion.

## Admit Owner Input

`admit` requires the input actor to own the target and to match the recorded
supplier of the text. It refuses a stale target or deleted work. Queue admission
always returns Queued, and Steer also queues while work cannot progress. A
successful admission does not establish that the host included the input in a
model request.

### Progress Boundary

`can_progress` requires an open lifecycle, no pause reasons, and an observed
execution state other than Prepared, Stopped, or Unknown. The host must still
apply the model and tool boundary checks. This predicate neither starts a
prepared child nor reconciles an uncertain effect.

## Commit Transitions

`transition` compares the complete work reference before copying the snapshot
and advancing its control revision. Revision exhaustion fails rather than
wrapping. The caller commits the returned state under its own serialization,
so two transitions cannot both commit against the same snapshot.

### Resume and Delete

Resume requires open work with no in-flight effects and removes only the
requested pause reasons. Delete requires paused, stopped, or prepared execution
with no in-flight effects. Deleting a task record also requires a retained
child. These checks preserve uncertainty until its source reconciles it.

### Hold Matching

`blocked_by` matches unreleased holds to the owner, agent, session, task, action,
and optional child set. Message dispatch needs an identified child. `release`
requires both the imposing principal and the coordinator's owner to match the
actor. Callers still serialize the match with the action they are about to send.

## Advance Receipts

`receipt_progress` accepts an unchanged stage and the declared forward
transitions. Delivered can become Included only after the host observes input
inclusion. Unknown and Partial can be reconciled with later evidence. A final
stage cannot silently return to Accepted and trigger execution again.

## Integration Boundary

The [interaction engine](../../../../../interactions/docs/more/libraries/interactions/interactions.md)
provides persistence and host coordination. [Interaction References](../../../../../interactions/contract/docs/more/libraries/interactions/interactions/contract.md)
defines the snapshots, selectors, operations, and receipt stages used here.
