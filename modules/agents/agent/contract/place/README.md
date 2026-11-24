# Host Contract

`link-agent-place` describes where an agent lives off your machines. The package holds
the `agent-*` commands an adapter answers when its account hosts
agents. The commands move an agent there and back, chat with the agent, and manage its backups
and its teams.

<!-- docs-tree:start -->

## Docs

- [Accounts That Host Agents](docs/agents/more/interface/contract/hosting.md): The commands an adapter (`lnk-<kind>`) answers when its account hosts agents

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): the commands, their request and reply
- [`teams.rs`](src/teams.rs): a hosted team's commands
