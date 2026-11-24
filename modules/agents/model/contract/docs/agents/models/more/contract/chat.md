# Model Caller Types

Share typed prompts, tool results, and completed responses between a harness
and the model bridge. `link_model_contract::chat` gives callers one message
representation while its optional client maps that representation to providers.

## Choose the Boundary

The [model action interface](../../../../../../docs/agents/models/more/api.md)
owns HTTP routes, grants, and command output. A library caller uses the types
below to construct or read those requests without duplicating their fields.
The `client` feature additionally exposes provider request and stream mapping.

### Borrowed Prompts

[`Prompt`](../../../../../src/chat.rs) borrows the system text, messages, tools, price jumps,
and prompt-section spans for a call. The caller supplies its answer and window
limits, thinking settings, and whether tools may be called. `chat` and `parent`
identify the work for context reporting rather than authorizing execution.

### Owned Requests

`Chat` owns the corresponding values for a serialized request and has an
optional model name. `Chat::prompt` borrows those values without rebuilding
history or changing the requested limits. Deserialization refuses unknown
request fields, while `Chat::check` performs the semantic checks below.

## Validate a Chat

Validation belongs at the request boundary before a provider sees the data.
`Chat::check` checks limits, offered tools, message roles, and media. It does
not execute tools or enforce a harness’s conversation-persistence policy.

### Answer and Window

The answer limit must be positive and fit within the requested window.
Those numbers describe this request. Model selection, known model limits,
and a caller’s budget remain responsibilities of the model bridge and harness.

### Tool Definitions

A `Tool` has a name, description, and object-valued JSON schema. Validation
requires a name of 1 to 128 ASCII letters, digits, underscores, or hyphens.
An offered definition tells the model what it may request. Permission to
execute that request still comes from the tool’s own grant.

### Message Roles and Parts

A `Message` has a `user` or `assistant` role, text, calls, answers, provider
thinking blocks, and media parts. Fields omitted from its serialized form
use their declared defaults. The checker reconstructs each media part through
[`Media::new`](../../../../../src/chat/media.rs) and refuses a kind that disagrees with its MIME type.

## Preserve Tool Continuation

A model’s `Call` names the requested tool and arguments with a call ID.
An `Answer` names the call it answers, its text, failure flag, and any media.
Keeping both structured records lets the next model request associate the
result with its request rather than treating it as an unrelated user message.

### Provider Continuation

`Message::thinking` and `Answered::thinking` retain provider-owned JSON blocks.
The caller preserves those blocks alongside visible text. Their meaning and
wire representation belong to the provider mapping; the shared message type
does not replace them with a summary or infer an execution effect from them.

### Persistence and Recovery

The message representation contains enough structure to retain calls and
results, but storing it is the harness’s responsibility. For example,
[Mini](../../../../../../../harnesses/mini/docs/agents/harnesses/more/mini/runtime/conversation.md)
checks for unanswered calls before another turn. The model client supplies
neither a conversation store nor a policy for replaying interrupted tools.

## Read a Completed Response

`Answered` holds visible text, requested calls, thinking, optional model and
response identifiers, token counts, latency, optional cost, and an interrupted
flag. A caller keeps the completed response separately from the text pieces
it displayed while the provider was still answering.

### Stream Events

`Stream` is tagged by `event`. A `text` event contributes displayed text;
an `answer` event contains the completed `Answered`; an `error` event contains
a failure kind and message. A client cannot infer a completed answer from
having received text alone.

### Failure Kinds

The shared failure vocabulary is `unreached`, `too_long`, `overloaded`, and
`refused`. These distinguish connection or initial-response failure, a prompt
outside the window, provider capacity failure, and other refusals. The caller
decides whether a fallback or another user action is appropriate.

## Use the Provider Client

With `client` enabled, [`chat::client`](../../../../../src/chat/client.rs) exposes the shared
provider mapping used by the bridge and standalone callers. Its `Model`
holds the caller-supplied provider, model name, base address, and credential.
Those backend values are separate from the serialized `Chat` request.

### Request and Response Mapping

The client builds Anthropic or OpenAI-compatible request bodies and reads
their streaming responses into `Answered`. Provider-specific request helpers
and reply decoders stay beside that client, so a harness can reuse them
without maintaining another provider protocol implementation.

### Cancellation and Initial Wait

`answer` accepts the caller’s cancellation receiver and a channel for text
pieces. `answer_within` additionally bounds the initial response wait when
the caller supplies one. Cancellation and timeouts concern that model call;
the caller still owns its saved turn, tool cleanup, and subsequent policy.

## Other Model Actions

[`api`](../../../../../src/api.rs) defines closed request and answer shapes for embeddings,
transcription, token counting, status, usage, and context reporting. `Embed`
contains texts and an optional model; `Transcribe` names a model and audio;
`Count` contains text and an optional model. Their answers carry vectors,
transcribed text, or a token count without provider-specific response fields.

### Read Operations

`Status` reports readiness and loaded model names. `Usage` reports request,
unmetered-request, and token totals. `ContextOf` optionally selects a chat;
`Context` returns retained chat metadata and an optional report. The bridge
owns authorization, operation availability, and the meaning of unavailable
results, as the [action interface](../../../../../../docs/agents/models/more/api.md) explains.
