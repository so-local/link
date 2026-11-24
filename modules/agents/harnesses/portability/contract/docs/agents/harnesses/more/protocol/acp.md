# Agent Client Protocol

The Agent Client Protocol (ACP) is an externally maintained open
standard for connecting clients to agents, letting a client prompt an
agent and receive its responses and tool activity through a common
interface. Link uses ACP to communicate with its harnesses.

## What ACP provides

A client can be an editor, a chat interface or a service forwarding
messages. The agent runs the model and tools. ACP defines the messages
between them, so clients and agents can interoperate without each
pair needing its own conversational API. The authoritative protocol
specification is maintained by the
[ACP project](https://agentclientprotocol.com).

### Sessions and responses

A client initializes the connection, discovers the agent's
capabilities and opens a session. Prompts go to that session, and the
agent streams updates describing its response and tool activity. A
client can cancel a running turn; loading, listing and resuming saved
sessions depend on the capabilities the agent advertises.

## How Link supports ACP

Link uses ACP's session lifecycle and types, through the protocol's
Rust SDK. Link Harness serves ACP version 1 over a WebSocket, and the
harness hub bridges ACP over stdio to Link's WebSocket clients. Link's
connections and chat interface use this shared conversation protocol.

### Capabilities and transport

Read the `initialize` response before using optional features. Link
Harness advertises session loading, listing and resuming, images and
embedded resources; another harness may support a different set. Link's
endpoint authentication and WebSocket framing are integration details,
documented with the concrete endpoints.
Link Harness's [ACP page](../../../../../../../link/docs/agents/link/more/acp/README.md)
describes its methods, capabilities and endpoint. The
[Harness Hub](../../../../../../hub/docs/agents/harnesses/more/protocol/hub.md)
describes how adapters expose an ACP harness through Link, and the
[harness interface](README.md) describes installation, configuration and
launch commands outside the ACP conversation protocol.

## Carrying state between harnesses

The [Agent Protocol](../agent-protocol.md) combines ACP's conversation
interface with Link's [Agent State Protocol (ASP)](../../../../../../agent-state-protocol/docs/agents/harnesses/more/agent-state-protocol.md).
ASP adds optional state transfer through ACP extension methods and
metadata. Together they let Link obtain portable state from one harness
and ask another to take it, subject to their advertised capabilities.

### For ACP implementers

ASP is Link's extension, not a requirement for an ACP implementation
or an adopted upstream ACP feature. Its state methods use the
`_asp/state/` namespace and its capabilities use `_meta.asp`. Support
for ordinary ACP sessions does not imply support for these extensions;
clients discover state export and import separately. Link-specific
`_link/` methods are documented with the endpoints that use them.
