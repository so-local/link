# Host Fixture

`lnk-agent-place-fixture` demonstrates how a host serves agents, as the [hosted-agent contract](../../../modules/agents/agent/contract/place/docs/agents/more/interface/contract/hosting.md)
describes, using inspectable files instead of running agents. Tests exercise moves,
backups, chat and refusals without a hosting account. It is never shipped.

## Code

- [`chat.rs`](src/chat.rs): streamed replies and chat controls for tests
- [`declared/`](declared/actions.toml): the agent place contract's `agent-*` commands it implements, which its command line is generated from
- [`host.rs`](src/host.rs): agent records, connections, inventory, backups and teams
- [`main.rs`](src/main.rs): request dispatch, logging and failure controls
- [`moves.rs`](src/moves.rs): copy, sealed switch, handover and abort
