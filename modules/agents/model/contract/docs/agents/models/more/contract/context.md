# Context Caller Types

Attribute a model request to stable prompt parts without saving its text.
`link_model_contract::context` supplies the census, call records, and report
shapes shared by the model bridge and callers that compose instructions.

## Identify Prompt Parts

The [`census`](../../../../../src/context/census.rs) reads a provider request into named parts.
Instruction text, tool definitions, messages, calls, results, and media remain
distinct categories. A part stores its path, the byte count measured by the
reader, and an estimated token weight rather than the original content.

### Stable Paths

Instruction parts use `instructions`, tool definitions use `tools/<name>`,
and conversation parts use paths beneath `conversation/`. The reader bounds
the distinct parts and tool-name length; names outside those limits fall
into the category’s `other` part. Callers should compare paths across calls
rather than infer categories from the order they were discovered.

### Instruction Spans

A caller that assembled the system prompt can supply `Span` values with a
name and byte range. [`split`](../../../../../src/context.rs) accepts ordered, non-overlapping
ranges on UTF-8 character boundaries with valid names. It reads the first
256 spans, leaving bytes outside their ranges in the general instructions
part. An invalid span among those read returns no split rather than
fabricating section boundaries.

## Record a Call

A `Call` records sequence, time, model, optional window, usage, and part
measurements for one request. The bridge owns writing and retaining those
records. Callers use the same forms for reports without creating another
copy of the prompt or a competing conversation history.

### Measurement and Estimation

Measured provider usage can be apportioned across parts according to their
weights. Without that measurement, the weights remain estimates. The report
retains the distinction; a part’s exact byte count does not make its token
estimate an exact tokenizer result.

### Cost

Cost uses the model facts and usage known for the call. Unknown pricing is
not a free call. [Context Reporting](../../../../../../docs/agents/models/context.md)
owns the user-visible report, including cached input and totals across a chat.

## Name Related Work

`Prompt::chat` identifies a call’s chat using the caller’s stable name.
`Prompt::parent` relates supporting work to the chat that requested it.
Those labels support grouping and totals; neither label authenticates a
caller or selects another agent’s records.

### Retained Chats

The public report types distinguish retained chat metadata from the selected
chat’s report. `api::ContextOf` requests a chat by name or lets the bridge
choose the latest one. The model bridge resolves the proved agent and its
record location before reading that report.

## Keep the Storage Boundary

`context::dir` resolves the record directory beneath the supplied settings
root and validates the agent identifier. `forget` removes that agent’s
records, treating an absent directory as nothing to remove. The helpers do
not authorize a caller to inspect or remove another agent’s records.

### Inspect the Implementation

[`context.rs`](../../../../../src/context.rs) owns the public types, span splitting, and
report calculations; [`census.rs`](../../../../../src/context/census.rs) owns provider-request reading.
The [model module](../../../../../../docs/agents/models/README.md) owns runtime
routing and the persistence policy described in its context-reporting guide.
