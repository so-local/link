# Harness Contract

`link-harness-contract` sets the rules between the agent module (`lnk-agent`) and a harness program. It defines the commands a harness program answers, how collections are stored, and the owner's one-shot decisions.

<!-- docs-tree:start -->

## Docs

### Packages

- [Harness Base](base/README.md): `link-harness-base` holds the vocabulary every harness shares apart from the host's orchestration
- [Harness Wire](wire/README.md): `link-harness-wire` holds the protocol-free vocabulary that a harness and its agent's host share

### More

- [Harness Contracts](docs/agents/harnesses/more/README.md): Implement or inspect the boundary between Local Link and a harness
  - [Agent Protocol](docs/agents/harnesses/more/agent-protocol.md): The Agent Protocol connects Link to the program running your agent and carries supported state when that program changes
  - [Harness Vocabulary](docs/agents/harnesses/more/vocabulary/README.md): Share the types a harness and host use without imposing a transport
  - [Interface](docs/agents/harnesses/more/protocol/README.md): A harness program, `lnk-<name>`, answers a set of commands and speaks typed conversations

<!-- docs-tree:end -->

## Code

- [`harness.rs`](src/harness.rs): the harness program's commands

- [`harness/authority.rs`](src/harness/authority.rs): session metadata and owner decisions
