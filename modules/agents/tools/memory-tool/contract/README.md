# Memory Contract

`link-memory-tool-contract` holds the agreement between the agent module and a memory engine's adapter for `lnk agent memory start <engine>`, and how a bound engine proves itself at an embedding endpoint.

<!-- docs-tree:start -->

## Docs

- [Interface](docs/agents/tools/memory/more/protocol.md): The commands a memory engine, `lnk-<engine>`, answers for an agent, typed in `link-memory-tool-contract`

<!-- docs-tree:end -->

## Code

- [`actions.toml`](actions.toml): the protocol's commands, declared
- [`checks.rs`](src/checks.rs): the check hooks the declarations name
- [`memory.rs`](src/memory.rs): the adapter contract's variables, session lines and bindings
- [`memory/embedding.rs`](src/memory/embedding.rs): the embedding endpoint's proof
- [`types.toml`](types.toml): what `command` prints and `serve-many` reads, declared
