# Interaction References

`link-interactions-contract` gives sources, hosts and renderers one versioned vocabulary for questions, bound effects, owner input, work controls and scheduled work. It depends on principal types and serialization, with no harness, scheduler, store or runtime dependency.

## Question Lifecycle

Requests, items, choices, deliveries and response events have distinct identifiers. Strict tagged enums carry confirmation, text and bounded choice answers. Sets retain partial submissions, recommendations, Other and exact free response. Resolution evidence distinguishes supplied wording, selecting owner and automatic fallback. Model proposals omit owner, domain, execution effect and continuation fields, which the trusted host supplies.

## Authority and Control

A bound operation identifies its source, schema, payload, review, scope and execution generation. Reply-only answers cannot mint it. Input and control envelopes carry their own IDs and owner-authenticated targets. Work references wrap host-issued session, task and attempt identities. Desired restrictions, observed execution and ordered receipts are separate data. Rule libraries validate transitions and the coordinator commits them.

## Schedule Identity

`ScheduleId` identifies a definition independently of its display name.
Native import binds the adapter and source job to one definition in the
managed agent. The portable schedule boundary validates the identity.

## Occurrence Identity

`ScheduledOccurrence` names the schedule, definition revision and
scheduled Unix instant of one firing. A delivery retry retains those
values, even when its retry deadline changes. Later recurrences and
changed definition revisions have distinct occurrence references.

## Versioned Envelopes

`delivery::Envelope` wraps a question, response candidate, owner input, or work
control with the protocol version. Its custom reader reads the version first,
through the interaction protocol its `protocol.toml` declares, and refuses an
older or a newer one by name before it reads the payload. Typed records reject unknown fields,
so an adapter must agree on a shape rather than guess at extra authority.

## Delivery Boundaries

`DomainDescriptor` identifies the source domain and whether it authorizes an
effect. `SurfaceCapabilities` declares supported answer shapes, sets, inline
presentation, and review size. The delivery adapter chooses a surface capable
of presenting the frozen question rather than truncating its review silently.

### Owner Response Codes

`DeliveryEnvelope` carries the response code only to an owner surface. Source
and model snapshots must not receive it. `ContextAnchor` binds inline controls
to a container, optional element, and content revision so a changed message
cannot retain authority from an obsolete presentation.

## Work and Receipt Records

`WorkState` separates lifecycle, observed execution, pause reasons, checkpoints,
and in-flight effects. A `ResolvedControlBatch` retains exact addressed members
and the lineage revision used to select them. Retries use that membership
instead of discovering newly created children.

### Ordered Evidence

`Receipt` carries a sequence, correlation, exact target, stage, and evidence.
Accepted, Delivered, and Included describe different observations. Unknown,
Partial, Denied, and Unsupported remain explicit outcomes so a transport's
successful delivery cannot imply execution or authority.

## Callers

The [interaction engine](../../../../../../docs/more/libraries/interactions/interactions.md)
coordinates these records. [Control Rules](../../../../../../../controls/docs/more/libraries/interactions/controls.md)
validates input admission and transitions, while [Approvals](../../../../../../../approvals/docs/more/libraries/interactions/approvals.md)
validates bound effects. The contract itself performs no dispatch and persists
no question or task state.
