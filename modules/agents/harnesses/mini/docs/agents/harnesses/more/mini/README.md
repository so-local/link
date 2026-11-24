# Mini Harness

> Mini Harness Makes the Agent Loop Visible

Mini Harness is a small, persistent agent that calls a model, runs its requested tools and remembers the conversation. Its Rust implementation makes the complete agent loop readable enough to embed, extend and explain.

<!-- quickstart -->

## Start with Mini

With Local Link installed and running, start an agent and configure its model through the normal setup. Select Mini to run that agent with the small text-and-tools harness.

```sh
lnk agent start
lnk agent harness set mini
```

### Send a Message

Send a first message through the terminal. The selected model supplies the answer, while Mini manages the conversation and any tool calls it requests.

```sh
lnk agent chat send --message "What does an agent harness do?"
```

### Give an Instruction

Put optional guidance in `instructions.md` in the agent's configured Home, such as `~/Link/Agents/main/home/instructions.md`. Mini reads it once before each turn and passes its text unchanged to the model. A missing file means no system instruction, and Mini adds no instruction of its own.

## Choose Mini

A harness is the program around a model that keeps its conversation and executes the tools it requests. Mini supplies that working loop, persistence and Link integration. It is useful when you want to see or reuse those responsibilities without first understanding a larger assistant's skills, planning and context management.

| Reader | Use Mini to |
|---|---|
| Developer | Embed, integrate and extend the Rust harness |
| Student | Explain every step of a turn and its failure paths |
| Teacher | Select modules and examples from the same references |

Mini also provides a small installed text agent with saved conversations and MCP tools, and a reference for contract tests and benchmarks.

### Know When Mini Is a Poor Fit

Mini is a poor fit for a personal assistant that needs skills, a composed persona, media input or automatic context management. [Link Harness](../../../../../../link/docs/agents/link/README.md) provides those capabilities. [Local Link's permissions](../../../../../../../agent/docs/security/sandbox/permissions/README.md) govern what the agent may reach. [Models](../../../../../../../model/docs/agents/models/README.md) supplies the common model interface and translates requests into provider protocols.

## Follow a Complete Turn

A turn is the work that follows one incoming user message. Follow a request to read `note.txt` from the instruction's source through the final reply, then identify which steps repeat.

### 1. Choose the Instruction

The owner writes guidance in the optional [instruction file](runtime/instructions.md). Local Link supplies its path from the agent's configured Home. When the turn runs, Mini reads the file once and uses that text as the system instruction for every model request in the turn. Without the file, there is no system instruction. The instruction is separate from the user's message and cannot grant permissions.

### 2. Receive and Save the Message

The user sends `Read note.txt.` through terminal chat or another client. Local Link delivers the prompt through the Agent Client Protocol (ACP). Mini selects the conversation and locks it so turns cannot interleave. It loads the saved messages, refuses an unanswered earlier tool call, reads the instruction and saves the new user message. The [runtime](runtime/README.md) owns the file operations.

### 3. Prepare and Ask the Model

Mini discovers the granted Model Context Protocol (MCP) tools, applies the conversation's restrictions and sorts their definitions. Its [model operation](link/models.md) sends the instruction, complete conversation and available definitions through Models, the model module. A definition tells the model what it can request, such as `files__read`. No tools is valid too. The model can answer directly.

### 4. Handle the Model's Answer

The model returns assistant text and, optionally, structured tool calls. For this example it requests `files__read` with the path `note.txt` and call ID `read-1`. Mini checks that call IDs are nonempty and unique, then saves the assistant message, including any provider continuation blocks. The model has requested a tool call. Mini must call the tool and return its result to the model.

### 5. Execute and Save Tool Results

Mini runs the requested calls one at a time through its [tool operation](link/tools.md). The file tool returns `hello` with the matching ID `read-1`. Mini checks the ID and saves that result before doing more work. A tool failure is saved as a failed answer for the model to handle. A model or storage error stops the turn, and an interrupted model answer stops it before tools run.

### 6. Continue or Finish

After saving every tool result, Mini asks the model again with the expanded conversation. The model can now say `The note says hello.` If that answer has no calls, Mini saves it and finishes. If it requests more tools, Mini handles them and asks again. Text streams to the client as it arrives, and tool sessions close when the turn succeeds or returns an error.

## Recognize the Agentic Loop

Asking the model, recording its answer, executing requested tools and returning their results is the **agentic loop**. A model produces a response. The harness supplies instructions, conversation state and executable operations, then uses that response to decide what happens next. This repeated exchange is what turns the model into an agent that can act and continue from the results.

### Read the Loop

[`core::run_turn`](../../../../../src/core/mod.rs) implements the repeating part of the turn above. The [core walkthrough](core/README.md#read-the-loop) shows the Rust after tracing the [worked transcript](../../../../../tests/fixtures/tool-turn.json), including the call `read-1` and its matching answer. Instructions and incoming messages are prepared before that function runs.

### Supply the Operations

The [Link adapter's call to `run_turn`](link/README.md#follow-the-supplied-operations) supplies `ask_model`, `call_tool` and `append_message`. Those operations connect the model, executable tools and conversation storage to the core's ordering. An application embedding the core can supply its own implementations without copying the loop.

## Build on Mini

The supplied operations let an application add behavior around the same loop. [Extend Mini](extensions.md) shows where context management, skills, and scheduled messages fit in the turn, and why context management can stand alone without skills. It also compares those responsibilities with Link Harness. Mini's current Rust interfaces support reuse, but Mini and Link Harness still have separate implementations and Mini does not load installable harness extensions.

## Saved Conversations

Mini saves complete typed messages as JSONL, one JSON message per line, under the harness home's `threads/` directory. User text, assistant text, tool-call IDs and arguments, tool results, failure flags and provider thinking or continuation blocks survive turns and restarts. It keeps the tool result even when the final reply does not quote it.

### Recover from Interrupted Work

The call is saved before execution and the answer before the next model request. A storage or model error stops the turn. If a later turn finds a call without its answer, Mini requires a new conversation instead of replaying a possibly completed side effect. Native moves preserve those files, while portable session export carries user and assistant text rather than the complete provider-specific state.

## Models and Caching

Mini asks the selected model through [Models](../../../../../../../model/docs/agents/models/README.md), which owns provider protocols, streaming and cache controls. Mini keeps earlier messages unchanged and sorts tool definitions, preserving the matching prompt prefix a provider can reuse. Anthropic cache markers and OpenAI's automatic prefix caching belong to that model layer.

### Token Cost

Caching can reduce repeated input cost, but reuse depends on the provider, model, prompt size and expiry. Mini promises no live cache-hit rate. Each answer is capped at 4,096 tokens, further limited by the model's known answer and context limits. Mini has no context compaction, automatic model fallback or spending controls, and a growing conversation can cost more or exceed its context window.

## Know Mini's Limits

Mini handles text and MCP tools. It has no skills, persona composition, instruction discovery, planner or slash-command system. Images and binary documents are unsupported. The loop has no hardcoded round cap, so a model can keep requesting tools until it finishes or the running agent is stopped. Tool failures go back to the model as failed answers, while model and storage failures end the turn.

## Learn the Harness

The pages below mirror the implementation's three folders. The [worked tool turn](../../../../../tests/fixtures/tool-turn.json) supplies a concrete transcript that the core tests execute, including a tool-call ID and its matching answer.

- [Core](core/README.md) owns `run_turn`, the model-and-tools loop. It accepts `ask_model`, `call_tool` and `append_message` operations from its caller.
- [Runtime](runtime/README.md) owns `load_conversation`, `append_message` and `read_instructions`. It accepts explicit paths and knows nothing about Link protocols.
- [Link Adapter](link/README.md) supplies the selected model, granted tools, agent configuration and protocol connections. Fixture and benchmark callers reuse that implementation.

### Design and Authority

Mini keeps a small loop on purpose, and [Decisions](decisions.md) gives
the reasons. Local Link's host and sandbox supply the authority Mini runs
with, as [Security](security.md) describes. Both boundaries matter when
you embed or extend Mini.

### Reuse the Operations

The six operation names above describe responsibilities rather than Rust syntax. Mini's [reusable boundaries](extensions.md) show where an application can add behavior and how the richer Link Harness compares.

<!-- docs-tree:start -->

## Docs

### Pages

- [Core](core/README.md): The core owns the repeated conversation between a model and its tools
- [Runtime](runtime/README.md): The runtime makes a conversation survive the end of one request
- [Link Adapter](link/README.md): Mini's Link adapter connects the small runtime to an installed agent
- [Extend Mini](extensions.md): Mini's core and runtime can be reused with different model, tool and recording operations

### More

- [Decisions](decisions.md): Mini is the smallest persistent, tool-using harness built on Link's model and tool interfaces
- [Security](security.md): Mini relies on Link's harness hub, model interface, tool bridge and sandbox for authentication and confinement

<!-- docs-tree:end -->
