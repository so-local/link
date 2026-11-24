# Security

Mini relies on Link's harness hub, model interface, tool bridge and sandbox for authentication and confinement. Its core implements no permission or credential policy.

## Endpoint and Tools

The hub authenticates requests for the agent before forwarding them to the Agent Client Protocol (ACP). A saved tool restriction can only narrow a conversation's available tools. An unreadable or malformed restriction fails the turn instead of granting all tools. Model-facing tool definitions contain no MCP endpoint addresses or session credentials.

## Stored State

Model configuration and endpoint authentication are saved privately in the harness home. That configuration and each conversation's saved tool restriction name their schema version, so a later release can upgrade them. Conversation files are private and contain complete user, assistant and tool messages, including tool outputs and required provider continuation blocks. Treat those files as sensitive conversation data. Imported session names cannot select paths outside the conversation folder.

### Instructions

The adapter supplies one instruction-file path from the agent's configured Home. Its contents guide the model without changing tool grants, credentials or sandbox permissions. The file is read once per turn, with other read errors reported rather than silently discarded.

### Portable Export

A native move preserves complete conversation files. Portable session export contains user and assistant text and omits credentials, tool restrictions, identity and memory. It does not promise provider-specific continuation in another harness.

## Interrupted Work

A model's call is recorded before execution, and its result before the model continues. A recording failure stops execution. Saved calls without answers cause a later turn to fail instead of repeating a possibly completed side effect. This is not an exactly-once execution guarantee, and Mini has no automatic recovery policy.

## Run Lifetime

Mini has no round budget, compaction or automatic fallback. A model can continue requesting tools until the caller stops the agent, and long conversations can exhaust its context window. Model errors end the turn. Link's permissions apply to every tool call.

<!-- docs-tree:start -->

## Docs

<!-- docs-tree:end -->
