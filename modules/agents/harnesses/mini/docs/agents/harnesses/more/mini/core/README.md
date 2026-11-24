# Core

The core owns the repeated conversation between a model and its tools. Its caller has already prepared the instruction and incoming message as described in [Follow a Complete Turn](../README.md#follow-a-complete-turn).

## Follow One Turn

Suppose the user asks what is in `note.txt`. The [executable example](../../../../../../tests/fixtures/tool-turn.json) describes these messages in JSON. A core test runs that same example and compares the complete transcript. The model assigns call ID `read-1`, and the tool answer repeats that ID.

| Step | What is recorded | What happens next |
|---|---|---|
| 1 | User asks to read `note.txt` | The model receives the conversation and available tools |
| 2 | Assistant requests `files__read` for `note.txt` | Mini records the request, then calls the tool |
| 3 | Tool answer says `hello`, referring to `read-1` | Mini records the answer, then asks the model again |
| 4 | Assistant says the note contains `hello`, with no tool calls | Mini records the message and finishes the turn |

### Why Ask Again

The first model answer can be a request for information. It cannot know the tool's result until the program supplies it. The next model call sees the original question, its own tool request and the answer together, so it can produce a useful reply.

### Why Keep the Order

The call is saved before the tool runs, and the result is saved before the model continues. A tool can have changed something even if saving its result fails. The [runtime](../runtime/conversation.md#handle-interrupted-work) refuses to replay an unanswered saved call, because the transcript alone cannot establish whether that side effect happened.

## Understand Messages and Calls

A `Message` records what one participant contributed. The user supplies text. The model supplies assistant text and can also supply structured tool calls. A `Call` has an ID, a tool name and arguments. It is a request to the program, not code the model executes itself.

### Tool Failures

A tool result is an `Answer` that refers to its call's ID. A failure has its failure flag set and is returned to the model, which can explain the problem or request another operation. A model error or recording error instead returns an error from `run_turn`, without an automatic retry.

## Read the Loop

[`core::run_turn`](../../../../../../src/core/mod.rs) is the whole loop. Its caller supplies `ask_model`, `call_tool` and `append_message` as functions, often called callbacks. The sections below trace the linked function’s outer loop, including validation, recording, and execution in their actual order.

### Receive a Model Answer

Each pass asks the model with all messages collected so far. Its answer becomes an assistant message, retaining tool calls and provider continuation data alongside visible text.


### Save Before Acting

The intervening source checks that call IDs are nonempty and unique. Recording then precedes execution, and an interrupted model answer ends the turn. An assistant message with no calls is the loop's normal stopping condition.


### Execute and Record Tools

The loop runs calls one at a time and checks each answer's ID. The tool result becomes a user-role message in its `answers` field, which distinguishes it from a person's text. The next pass asks the model again with the saved results.


## Keep the Core Small

The core has no system instruction, round limit, provider protocol, session directory, skill loader or permission policy. It accepts the conversation and the operations it needs. The [runtime](../runtime/README.md) provides persistence, while [Link integration](../link/README.md) connects the model and tools.

### Model Continuation Data

Some models return signed thinking or other continuation blocks alongside their text. The loop keeps those blocks without interpreting or rewriting them. Understanding their provider-specific format belongs to [Models](../../../../../../../../model/docs/agents/models/README.md). Keeping them preserves both valid continuation and reusable cached context.

## Reference Functions

`run_turn` owns the ordering above. The supplied operations make that ordering reusable for a different model connection, tool implementation or recorder without changing the loop.

- `ask_model` accepts the current messages and returns the model's text, calls and continuation data.
- `call_tool` accepts one call and returns its matching answer, including a failure when execution cannot complete.
- `append_message` records one complete message before the next operation and can stop the loop by returning an error.

## Read the Rust Signature

The Rust signature uses `Future` for operations that finish asynchronously, such as waiting for a network response. `M` and `T` are the supplied model and tool functions, and `Asked` and `Called` are their returned futures. The [source signature](../../../../../../src/core/mod.rs) states their types, while the worked transcript defines their observable ordering.
