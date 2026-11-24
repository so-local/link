# Harness Hub

`link-harness-hub` serves a harness's Agent Client Protocol (ACP) over stdio to Local Link's bridges as ACP over a WebSocket, with each thread a session kept across restarts. Every harness program's relay is built on it.

<!-- docs-tree:start -->

## Docs

### Pages

- [Harness Hub](docs/agents/harnesses/more/protocol/hub.md): `link-harness-hub` serves a harness that speaks the Agent Client Protocol (ACP) over stdio to Link's bridges, as Link Harness serves its own: ACP over a WebSocket at the endpoint's `/acp`, opened with the agent's headers and its endpoint…

### More

- [Decisions](docs/agents/harnesses/more/protocol/hub/decisions.md): Why the harness hub maps Link's threads onto a harness's sessions and carries agent state through the same endpoint

<!-- docs-tree:end -->

## Code

- [`activity.rs`](src/activity.rs): session operation ownership and exclusive capture
- [`harness.rs`](src/harness.rs): the harness behind it, started and started again
- [`lib.rs`](src/lib.rs): the hub, its endpoint and its key
- [`serving.rs`](src/serving.rs): a bridge's connection, each thread a session
