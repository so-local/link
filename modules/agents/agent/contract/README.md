# Agent Contract

`link-agent-contract` defines what an agent is, for every program that reads
an agent without running it. The package lays out an agent's folder, says who the
agent is and whose, holds the agent's sandbox file, and defines the stream an
agent moves in. `lnk agent state-send`, `receive`, and a cloud agree on that stream
byte for byte.

<!-- docs-tree:start -->

## Docs

### Packages

- [Agent Identity](identity/README.md): `link-agent-identity` holds agent IDs, owners, and who outside may do what, without a running agent
- [Agent Specification](spec/README.md): `link-agent-spec` defines an agent's specification, the TOML file of what the agent is and its opinions
- [Agent Team](team/README.md): `link-agent-team` reads a team from its file
- [Host Contract](place/README.md): `link-agent-place` describes where an agent lives off your machines

### More

- [Agent Contract](docs/agents/more/interface/contract/README.md): The agent contract gives callers one definition of an agent's folders, identity, namespace actions, and transfer stream
  - [Move Stream](docs/agents/more/interface/contract/move-stream.md): The stream an agent moves in, as every end of it reads and writes it: `lnk agent state-send` and `receive`, a cloud that hosts agents (`agent-receive`, `agent-send`), and `lnk storage` for such an agent's Home…

<!-- docs-tree:end -->

## Code

- [`agent_sandbox.rs`](src/agent_sandbox.rs): its sandbox file
- [`folder.rs`](src/folder.rs): an agent's folder
- [`moving.rs`](src/moving.rs): the stream an agent moves in
