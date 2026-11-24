# Link Adapter

> Link Connects the Loop to an Agent

Mini's Link adapter connects the small runtime to an installed agent. It owns the commands every harness answers, the Agent Client Protocol (ACP) sessions, and the model and tool routes. That leaves the core independent of those protocols.

## Read the Adapter

[`link::run`](../../../../../../src/link/mod.rs) handles the adapter commands that install and configure Mini. [`link::agent`](../../../../../../src/link/agent.rs) serializes turns within each conversation, loads its state, opens its granted tools and invokes the core. The adapter keeps model settings and endpoint authentication in private configuration.

### Separate Concurrent Turns

The adapter keeps a lock for each conversation. In `Harness::turn`, the acquired lock remains in scope while the turn loads, appends and runs, so two prompts for the same conversation cannot interleave their records. The core itself has no concurrency or session policy.

### Answer Agent Client Protocol Sessions

[`link::acp`](../../../../../../src/link/acp.rs) accepts sessions and prompts, then streams model text back to the caller. Local Link's shared harness hub provides the authenticated endpoint. Portable session export and import are implemented by [`link::state`](../../../../../../src/link/state.rs), while [`link::threads`](../../../../../../src/link/threads.rs) supplies conversation paths and saved tool restrictions.

### Tools and Restrictions

Local Link supplies Model Context Protocol (MCP) servers for the tools the agent may use. The adapter discovers those tools, applies the conversation's saved restrictions and offers the remaining definitions to the model. Unreachable servers are left out of that turn, and a malformed saved restriction ends it. Restrictions can narrow the tool set but never widen it.

## Follow the Supplied Operations

After [preparing the turn](../README.md#follow-a-complete-turn), [`link::agent`](../../../../../../src/link/agent.rs) supplies the core's three operations. The model chooses what to request, the tool connection performs it, and the recorder preserves the result. The core controls their order. Tool sessions close after the call returns, including on a model or storage error.

## Model and Tool Operations

A model request travels through Local Link's provider interface, which
[Models](models.md) follows step by step. Tool discovery, conversation
restrictions, and the return of a requested operation's result to the
loop are in [Tools](tools.md).

## Separate the Adapter from Extensions

The folder name `link` identifies this adapter's host. It does not mean each Rust module in it is required by the core or is a harness extension. Model access is needed to answer a message, while MCP tools can be absent. ACP, agent configuration, and Local Link's installation commands belong to this adapter. [Extend Mini](../extensions.md) explains these boundaries and how additional capabilities relate to them.

<!-- docs-tree:start -->

## Docs

- [Models](models.md): Mini asks Models for an answer in one typed shape
- [Tools](tools.md): A tool is an operation the program can execute for the model

<!-- docs-tree:end -->
