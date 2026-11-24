# Agent Protocol

The Agent Protocol connects Link to the program running your agent and carries supported state when that program changes. It brings the Agent Client Protocol (ACP) and Agent State Protocol (ASP) together so the same agent commands can manage different harnesses.

## Agent and Harness

An agent has an identity, configuration, conversations, knowledge, and files. A harness is the program around its model that keeps conversations and calls tools. Link can change the harness, move the agent to another machine, or adopt an existing installation while keeping track of which state belongs to the agent.

## Agent Client Protocol

ACP is an externally maintained open standard for communicating with an agent. A client discovers capabilities, opens or resumes a session, sends prompts, and receives streamed answers and tool activity. Link uses this common interface for terminal chat and messaging connections. [ACP](protocol/acp.md) explains the session lifecycle and where to find the upstream specification.

### Conversation Content

A prompt can include text and supported images or embedded documents. A session can replay the history its harness exposes. Sending a document's contents or replaying a conversation does not synchronize a project directory or copy the harness's installation.

## Agent State Protocol

ASP is Link's optional extension to ACP for describing, exporting, and importing state. The source advertises what it exports, and the destination separately advertises what it imports. Import reports what was taken, what could not be taken and why, and the session identifiers to use next. [ASP](../../../../../agent-state-protocol/docs/agents/harnesses/more/agent-state-protocol.md) defines the methods and representations.

### Portable State

Portable state represents conversations as ACP updates, instructions as `AGENTS.md`, skills as Agent Skills files, tools as Model Context Protocol (MCP) server descriptors, memory as sourced Markdown notes, settings as session option values, and schedules as definitions. Each kind is optional. An omitted kind means it was not supplied, which differs from supplying an empty collection.

### Transfer Capabilities

Export and import support describe separate directions. A destination may reconstruct supported content in native history, or retain the archive and supply selected history that fits the model's context budget on the next turn. Retention does not establish that every native feature was restored or that a model consumed the archive. The [harness comparison](../../../../../../../../../interop/agents/harnesses/docs/agents/harnesses/compare.md) explains the current adapters.

### Prompts During a Turn

ASP also lets a prompt request steering into the current answer, queueing for a later turn, or interruption of the current turn. This capability is advertised independently of state transfer. A harness refuses a requested behavior it does not support.

## Contracts and Adapters

A contract declares the operations, types, and capabilities two parts agree on. An adapter is the program that implements that contract for a particular harness. Link asks for an operation through the shared interface, and the adapter handles its harness's configuration, native files, and process lifecycle.

### Harness Management

The [adapter interface](protocol/README.md) covers installation, configuration, launch, health, permissions, and native-state inventory around the Agent Protocol. Link Harness implements ACP and ASP itself. Other harnesses can expose ACP through their own entrypoint or an external adapter, with the [hub](../../../../../hub/docs/agents/harnesses/more/protocol/hub.md) providing Link's endpoint and session routing.

### Native State Translation

A harness implementing ASP can answer its state methods directly. Otherwise its adapter reads supported native state, or the hub obtains exposed conversations through ACP's session listing and replay. Native formats stay in the adapter. Capabilities describe the integration Link provides, which can be broader than the upstream program's own protocol support.

## State and Files

An agent's complete managed state includes more than its portable representation. The command performing the change decides which resources must be copied, restored, or configured, using the relevant contracts. These distinctions explain how the same harness can support a machine move while lacking conversation import from another harness.

| Resource | How Link handles it |
|---|---|
| Portable state | ASP representations and explicit import results |
| Native harness state | Adapter-declared files and native store operations |
| Workspace files | Existing access or a separate file transfer |
| Installed programs | Installation for the destination machine |
| Credentials and permissions | Separate secret and host authorization mechanisms |

### History and Authority

Historical tool calls remain data and never execute because they were imported. Source labels and transcript roles grant no permission. Memory corrections and explicit deletions retain their source, so an older snapshot cannot silently revive deleted notes. [Portable Context](protocol/continuity.md) specifies storage, revisions, and continuation boundaries.

## Use, Move, and Migrate

These commands manage different changes around the same agent, its harness, and their declared capabilities. Each uses the adapter's management contract. Once started, the managed harness is reached through ACP. The state path depends on what changes, with ASP providing portable transfer where supported.

### Switch the Harness

[`lnk agent harness set`](../../../../../../../agent/docs/agents/harnesses/use/README.md#switch-in-one-command) selects another harness and retains the agent's shared configuration and files. Link captures supported portable state, starts the destination through its adapter, and imports supported conversations before admitting waiting messages. Unsupported state is named, and each harness's native state remains available for a return.

### Move the Machine

[`lnk agent move`](../../../../../../../agent/docs/agents/move/README.md#move-your-agent-to-the-cloud) carries workspace files, agent collections, and adapter-declared native state to another machine, then installs and configures the harness there. Native state can resume on the same harness without converting every record through ASP. The move's stream and credential handling are separate from ACP's conversation messages.

### Adopt an Installation

[`lnk agent migrate`](../../../../../../../agent/docs/agents/README.md#migrate-your-agent) discovers a supported existing installation through its adapter, copies native state, relocates paths, handles supported secrets, and keeps its original service stopped. It runs the same harness under Link. Converting the agent to another harness is a subsequent switch with that pair's transfer capabilities.

## Extend the Adapters

Contributors can add another harness through the adapter interface and [conformance kit](../../../../../conformance/docs/agents/harnesses/more/protocol/conformance.md). Harness creators can implement ACP and ASP to expose control and supported state directly. Both routes let Link use declared capabilities while keeping native differences inside their integration.
