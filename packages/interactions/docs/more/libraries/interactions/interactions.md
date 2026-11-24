# Interactions

> Interactions Keep Every Answer in Its Place

Link Interactions coordinates questions, bound approvals and owner control intents through one protected store. Trusted hosts use the library while delivery adapters render the same immutable question on owner surfaces.

## Questions and Responses

A request contains an ordered set of confirmation, text or bounded choice items. Items have stable IDs, recommendations, optional Other answers and typed defaults. A submitted response can answer any subset, include item notes or contain only overall clarification. Skipped items stay skipped after submission. Recommendations and editable controls never submit an answer by themselves.

### Target a Response

An owner response must identify its delivered question by a recorded platform reply or code-prefixed text. A code alone answers only a singleton confirmation. Ordinary messages, including an unmarked `yes`, remain conversation. The ingress adapter authenticates the owner and supplies the route. The engine checks that route against a recorded delivery, along with the recipient, revision and deadline. Split deliveries can bind each platform message to the same question.

### Preserve Wording

Results retain supplied option wording, its supplier, the recommendation flag and the selecting owner separately. Other text, item notes and overall clarification preserve the user's exact text and input modality. Selecting generated wording does not make its author the user. Automatic fallback results have no selecting owner and never become owner responses.

### Resolve Deadlines

A finite deadline is frozen when the question is created, including delivery time. Information questions may expire, use typed defaults or return marked recommendations. Permission questions always fail closed. A partial submission resolves the entire set atomically and cannot later acquire timeout answers. Editing requires cancellation and a new request, code and review.

### Drafts and Inline Anchors

Each delivery has its own owner-bound draft and optimistic revision. Editing changes no resolution or authority. Explicit Submit atomically sends that revision, including skipped items and commentary. Inline controls bind the immutable question element's content revision; edits invalidate its deliveries without freezing unrelated streaming text. Adapters without inline controls use standalone forms. The first accepted response closes every presentation.

### Native Forms

Native dialogs show the frozen review before opening an empty answer form. Multiline text supports the same question-set directives and qualifications as CLI/chat fallback; nothing is selected automatically. Linux uses Zenity or KDialog, and macOS uses AppleScript. macOS question sets and reviews above 32 KiB use full CLI/chat delivery. Missing desktop capabilities leave those routes available. A winning response, deadline or shutdown closes the active dialog.

### Declared Capabilities

The built-in catalogue declares applicable source shapes and delivery surfaces on Linux and macOS. Agent questions support confirmation, text and choice; existing sandbox, owner-configuration, file-review and webhook sources ask confirmation only. Terminal, CLI chat, Link chat and the four messaging connections support text forms and sets. All current adapters present standalone forms; inline anchors remain available to adapters that declare them. Hosted, website and other operating-system adapters require their own capability declarations.

## Bound Effects

Reply-only questions have no permission state. Authorization requests contain one confirmation and an immutable source-owned operation, payload digest, semantic review and scope. The engine validates their binding through [Link Approvals](../../../../../approvals/docs/more/libraries/interactions/approvals.md). A positive response with accompanying qualification requires clarification before any claim.

### Claim and Report

The source keeps its typed payload and execution gate. It rechecks authority, the exact payload, attempt, deadline and control fence, then consumes a single claim under its dispatch serialization. A claim without an outcome remains applying or unknown and cannot be replayed. Applied, failed and unknown outcomes are distinct from the owner's accepted answer. External consent returns the answer without certifying external execution.

### Existing Policy

Existing-policy calls also record execution and uncertain outcomes without adding a question. Refusals and qualifications retain the complete typed resolution for the requester.

## Input and Control Intents

Owner inputs and lifecycle controls share identity, outbox events and receipts with questions. [Link Controls](../../../../../controls/docs/more/libraries/interactions/controls.md) resolves selectors and computes semantic transitions. The engine records the exact target list once, preserves per-target receipts and invalidates affected unclaimed approvals. Recording intent does not establish that a runtime has paused, included input or stopped.

## A State File from an Older Link

The state file names its schema, and an unsupported shape is refused.
The file holds questions, effects, control records, deliveries, and subscriber
cursors together. Preserve an unreadable file for diagnosis before starting a
new store, and reconcile outstanding effects with their sources rather than
assuming a replacement store has cancelled them.

## Storage and Recovery

Work coordination retains host-issued references and observed state. The host owns conversations and checkpoints, while the interaction store owns question records, control receipts and the bindings that sources validate before dispatch.

### Host Work Bindings

The host registers its exact conversation and attempt after taking its execution lease. Tools receive an unguessable binding outside model arguments. Question sources validate it against the current work, agent and attempt. Replacement attempts retire older bindings, revoke their unclaimed permissions and preserve child lineage. The shared dispatch lock orders control admission against source claims and provider writes; it never spans an owner wait.

### Where a Harness Hosts Work

`Host` is the one definition of the store a harness binds its work in. It is the machine's store when `lnk-agent` names it in `LNK_INTERACTIONS`, which it does for a harness that runs unsandboxed and so reaches Link's settings. No sandbox reaches them, so a sandboxed harness binds its work in its own home's store and its tool calls carry no binding. Its sources then see each call as one of no tracked work, under their own policy, rather than refusing an unknown binding.

### Pause and Retained Delegation

The Link Harness checks model and tool boundaries against desired controls. Ordinary pause permits the current model response to finish and blocks its follow-on calls. Immediate interruption cancels the active step. Checkpoints remain in the host conversation. Scheduler proposals bind exact jobs to a plan revision; active owner holds block dispatch. Releasing a hold leaves retained proposals awaiting explicit revalidation, and only the owner can release that hold.

### Inputs and Receipts

Queue joins a new turn; Steer enters the next safe model boundary and fences tools generated under older instructions. Inputs retain ingress order and original owner provenance. Delivered means written to the transcript; Included requires a completed provider request containing that exact input. Redirect stops the old path before promoting its recorded input. An input received while paused waits for explicit resumption.

### Retention and Deletion

Prepared children have stable identities before source dispatch. Selective controls preserve their parent relationship, original task and return destination. Child results remain durable until their destination accepts them. Stable delivery identities prevent retries from repeating parent input or restarting completed work. Archive retains progress after scoped shutdown; Unarchive leaves execution paused.

### Scoped Deletion

Delete requires a stopped, reconciled target and an explicit conversation, task-record or artifact scope. Conversation and task deletion wait for queued deliveries as well as host data; task deletion also waits for its scheduler record. Bulk receipts retain their recorded membership and report partial outcomes without selecting new work on retry.

### Unknown Effects

A claimed operation remains tracked until its source confirms Applied or Failed. Lost outcomes prevent automatic replay and prevent the runtime from reporting a false completed stop. Source reconciliation may settle the original claim after restart; it never creates a second execution claim. Tool policy changes and owner controls serialize with source dispatch, while approvals remain outside model arguments.

### Protected Transactions

The machine store is `~/.lnk/safe/approvals/`, outside runtime mounts. Request creation, responses, expiry, claims and event commits use an exclusive directory lock. Work, input, receipt and cursor observations use a shared lock without rewriting state. Writes sync a private temporary file, atomically rename it and sync the directory, so every write is a new file. Unknown versions, malformed data and linked state fail closed. The library reads no legacy question files or approval callbacks.

### Idempotency and Limits

A source idempotency key returns its original request while that question is kept, and refuses changed content. Once the question has left, the same key asks again. Request IDs and owner-only response codes are separate. A code is reserved within the store namespace while its question is kept. State is bounded to 64 MiB, 10,000 request records and 100,000 events. Reaching capacity refuses a transaction. What the store keeps of finished work is in [What the Store Keeps](#what-the-store-keeps). Notification/result redelivery never repeats an effect, and acknowledged subscriber cursors survive restart.

### Owner Presence

Live route guards use kernel locks and disappear when their owning process exits. They help the host preserve initiating-connection routing and direct-chat fallback. These route hints supply neither an accepted response nor execution authority. Response codes are rendered only for owner delivery and are excluded from source snapshots and model results.

## What the Store Keeps

Every write drops what no later call reads, so a turn costs the same after ten thousand turns as after one. Each rule follows the calls that read a collection. Nothing a pending operation, an open question, an undelivered input or an unread event needs ever leaves.

### Work Bindings

A retired binding leaves once a newer attempt of the same work holds its place. It stays while an effect under it is in flight or unknown, an unread event names it, other work names it as parent, or a question under it is open or applying. Its attempt then reads as stale through that newer binding, as it did while kept.

### Policy Calls

The record of a source's policy-checked call leaves with its attempt's binding. It guarded the call against a replay, which the stale attempt now refuses first.

### Outbox Events

An event leaves once every subscriber's cursor has passed it. With no subscriber, no event waits. The newest event always stays, because the next event's number follows it. A subscriber that starts afterwards begins at the first event kept, so the question each kept event names stays with it.

### A Subscriber That Never Reads Again

A subscriber that never reads again leaves with `unsubscribe`, and the events only it would read leave in that write. An owner delivery subscribes under a name of its own, and its host unsubscribes it when what it delivers for is deleted or leaves the machine. One that's only stopped stays subscribed, so the outbox keeps what it reads when it returns.

### Settled Inputs and Controls

An input settles once the model has read it, its receipt saying Included or RedirectApplied. A control batch settles once none of its members addresses a binding still kept, waits on a deletion, or pauses work. The store keeps the newest 256 settled inputs and batches with their receipts, so a retried admission is answered with its first receipt. Receipt numbers continue after those that leave. The number is `settled` in the library's `src/retention.toml`.

### Resolved Questions

A question is resolved once it's answered, expires, or is cancelled or closed. It stays 30 days after its resolution, with its response code, deliveries, their platform messages and drafts, so a late reply is answered AlreadyResolved or Expired rather than as an unknown question. Then it leaves at the next write, and its code may be given to another question. The days are `resolved_days` in the library's `src/retention.toml`, counted by the machine's clock from the write that resolved it.

#### What Holds a Resolved Question

A question stays however old while it's unresolved, its effect waits for a claim or is under way, or a kept event names it. A delivery that never reached its owner doesn't keep a resolved question: nothing delivers a resolved question.

### What Stays

Holds, proposals and child lineage stay. A long conversation adds none of them. Questions are bounded by their days after resolution and by the 10,000-record limit, which refuses a new question while that many are kept.

### Reads of an Unchanged File

Every write replaces the state file by rename, so a new write is a new file. A store keeps the file it last read open, with the state it parsed. Its number on the disk can't pass to another file while it's open. A read parses again only when the path holds another file, or the same file with another size or modification time. A change that leaves the state's bytes as they were writes nothing.

### Measured Cost

In a release build on one machine, bound turns grew the state before these rules: to 504 KB after 1,000 turns, with each read taking 11 to 13 ms, and to 5.5 MB after 10,000, with each read taking 130 to 144 ms and a turn 1.4 s. With them, the state stayed at 848 bytes after 10,000 turns. A read of the unchanged file took 5 to 11 microseconds, and a whole turn took 1 to 2 ms, most of it syncing its two writes to disk.

## Subagents in a Store of Its Own

A host whose store no other source shares, such as a sandboxed harness
with its own, never sees the scheduler's delegation. Its scheduler names
the parent conversation with a subagent's firing, and the host records
the subagent as a child of that conversation's current work before
admitting it. A child already recorded, or a retry, is left as it is,
and a parent with no work here is refused.

## Related Contracts

[Interaction References](../../../../contract/docs/more/libraries/interactions/interactions/contract.md)
defines the shared envelopes. [Decisions](interactions/decisions.md) explains the
coordination choices, and [Security](interactions/security.md) states the authority
and persistence guarantees callers must preserve.

## Caller Workflow

Choose a machine store or an explicit isolated folder, then create a request,
plan its owner delivery, and observe its committed resolution. Keep source
execution outside the delivery adapter and outside a transaction's lock.
Keep one store folder for the host’s lifetime: a replacement has its own
request IDs and owner codes, so old replies cannot resolve its questions.

### Create Idempotently

`create` makes a reply-only question. `create_bound` also accepts the domain,
effect, and semantic permission review. The idempotency namespace includes
source, principal, work, tool call, and the caller's key. Reusing a key with
changed content is refused, while an unchanged retry returns the original ID.

### Plan a Delivery

`plan_delivery` checks the deadline and surface capabilities before returning
an owner envelope. A retry for the same request, route, and surface reuses the
delivery identity. Record platform message IDs with `delivered_ids` after the
transport accepts them so a later platform reply can target that delivery.

### Submit Authenticated Responses

`submit` receives trusted ingress separately from the response candidate.
The result distinguishes Accepted, AlreadyResolved, and Expired. A transport
must retain that disposition rather than treating every successful network
submission as the question's winning answer.

### Observe Deadline Resolution

`snapshot` and `pending` apply deadline resolution before returning records.
They can therefore commit expiration even when the caller intends to inspect
state. `pending` returns unresolved questions, while a completed snapshot also
carries the effect state needed by an authorization source.

### Consume the Outbox

Register with `subscribe`, which answers the subscriber's saved cursor, then
read `events` after it, handle the events, and `acknowledge` the committed
event ID. The outbox keeps events only for subscribers it knows, until each
has acknowledged them. Acknowledgements cannot move backwards
or name an event beyond the store's outbox. Stable subscriber names retain
progress across process restarts without repeating a source effect.

### Cancel a Waiting Source

`wait` polls committed snapshots and checks a cancellation callback between
observations. Cancellation closes the request as its original requester.
A source that stops waiting must still report or reconcile an already claimed
effect, because closing delivery alone does not undo external execution.

### Handle Delivery Failure

Creating a delivery plan records its identity before the transport sends
it. A failed transport does not turn the question into an accepted answer.
Retry that delivery and record only the platform message IDs the transport
actually accepted; inventing an ID would let an unrelated reply target it.

### Separate Receipt from Execution

A subscriber may acknowledge handling an outbox event before an external
effect finishes. The source still owns its execution claim and outcome.
After restart, reconcile that claim before attempting the same operation,
because advancing a cursor supplies no evidence that a side effect ran.
