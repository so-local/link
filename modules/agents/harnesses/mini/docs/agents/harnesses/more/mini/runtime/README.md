# Runtime

> Runtime Keeps Conversations Complete

The runtime makes a conversation survive the end of one request. It supplies complete message storage and one optional instruction file without depending on the Agent Client Protocol (ACP) or a provider's API.

## Read the Runtime

[`runtime::conversation`](../../../../../../src/runtime/conversation.rs) locates, loads and appends conversation files. [`runtime::instructions`](../../../../../../src/runtime/instructions.rs) reads the instruction before a turn. Both accept explicit paths, so an embedding application can use them without an ACP session or a running Local Link agent.

### Begin a Turn

The [Link adapter](../../../../../../src/link/agent.rs) uses these operations before entering the core. Its `threads::read` helper calls `load_conversation`, and `threads::append` builds a user message and calls `append_message`. The instruction is read once and remains the same throughout the turn.


### Store Conversations and Read Instructions

Complete message records, and recovery after an interrupted write, are
defined in [Conversation Storage](conversation.md). One optional file
supplies the turn's system instruction, as [Instructions](instructions.md)
explains.

### Continue a Conversation

Each completed assistant message and each tool answer is appended as the loop produces it. The next turn reloads those records in the same order. Mini does not replace tool history with the assistant's final text or summarize old messages.

<!-- docs-tree:start -->

## Docs

- [Conversation Storage](conversation.md): A Mini conversation is one JSONL file, with one complete model-interface message on each line
- [Instructions](instructions.md): Mini reads one optional `instructions.md` file in the agent's Home

<!-- docs-tree:end -->
