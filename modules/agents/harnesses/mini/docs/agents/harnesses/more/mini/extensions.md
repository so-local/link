# Extend Mini

Mini's core and runtime can be reused with different model, tool and recording operations. These boundaries explain how to add behavior around the same agentic loop and which capabilities distinguish Link Harness.

## Start with the Same Turn

Keep the sequence in [Follow a Complete Turn](README.md#follow-a-complete-turn) visible as you add behavior. A caller prepares the instruction and incoming message, supplies the model and tool operations, then records their results. The core controls when those operations run. Mini and Link Harness currently have separate implementations, so this reuse boundary is a Rust interface, not an installable harness-extension system.

### Required Responsibilities

The core needs a conversation and operations for asking the model, executing any requested calls and recording messages. Its caller decides where instructions and messages come from and how they are stored. The [runtime](runtime/README.md) provides Mini's file-based implementation. The loop knows neither the directory layout nor the client protocol.

### Optional Tools

A conversation can have no tools. The model operation offers an empty definition list, the model returns text, and the turn ends without calling the tool operation. The core's signature still requires that operation, but an application without tools can supply one that returns a failed answer for any unexpected call. Tool availability and the loop's ability to handle calls are separate concerns.

## Locate the Integration Boundary

Mini's [`src/link/`](../../../../../src/link/mod.rs) folder connects reusable code to an installed Link agent. It supplies the selected model, granted MCP servers, conversation paths and Agent Client Protocol (ACP) connection. It is an adapter, not a folder of independently installable extensions. Embedding the core and runtime in another application does not require that application to use Mini's Link adapter.

### Model Connection

The caller supplies `ask_model`. Mini's implementation uses [Models](../../../../../../../model/docs/agents/models/README.md) for typed requests, provider protocols, streaming and cache controls. Reusing that implementation avoids reproducing provider behavior in each harness. A different caller can supply another connection while keeping the core's message and answer types.

### Tool Connection

The caller supplies `call_tool`. Mini uses [MCP tools](link/tools.md), but the core accepts an operation returning a typed answer without knowing MCP. Link's [permissions](../../../../../../../agent/docs/tooling/sandbox/permissions/README.md) still determine what an installed agent can reach. Adding a tool definition or an instruction does not authorize a tool call.

## Add Capabilities around the Loop

The following examples locate additional responsibilities in a turn. They describe places to compose behavior, not extensions Mini includes. Link Harness supplies the richer behavior through its own implementation.

### Context Management

Before each model request, a caller can prepare the context sent by its model operation. A compaction implementation also needs durable summaries and recovery behavior while preserving valid tool-call pairs and provider continuation data. This responsibility does not require a skill loader. Mini sends the complete conversation unchanged, while [Link Harness manages its window and summaries](../../../../../../link/docs/agents/harnesses/link/conversations.md#how-much-it-remembers).

### Skills and Instructions

An instruction composer can add a skill catalogue when preparing the model request. It must also provide a way to read the selected skill. Mini reads one instruction file, while [Link Harness lists skills](../../../../../../link/docs/agents/harnesses/link/guide.md#give-it-skills) and lets the model read their contents as needed. This adds instruction behavior around the loop rather than changing the order of model requests and tool results.

### Scheduled Messages

A scheduler supplies an incoming message when a trigger fires. Delivery still needs conversation routing, source attribution and rules for an already-running turn. Link Harness documents [messages from schedules and subagents](../../../../../../link/docs/agents/harnesses/link/conversations.md#messages-from-its-schedules-and-subagents). Scheduling is a source of work for an agent, distinct from the loop that carries out that work.

### Richer Input and State

Media handling prepares model-readable input from images, documents or audio. Saved-state handling determines what survives a restart, fork or move. [Link Harness media](../../../../../../link/docs/agents/harnesses/link/media/README.md) and [conversations](../../../../../../link/docs/agents/harnesses/link/conversations.md) describe these responsibilities. Reusing the small loop alone does not supply them or make its saved state compatible with another harness.

<!-- docs-tree:start -->

## Docs

<!-- docs-tree:end -->
