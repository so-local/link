# Mini Harness

Mini Harness is a small persistent, tool-using agent built on Local Link's model and tool interfaces. It runs as `lnk-mini-harness` and is the reference harness that tests and benchmarks use.

<!-- docs-tree:start -->

## Docs

### Pages

- [Mini Harness](docs/agents/harnesses/more/mini/README.md): Mini Harness is a small, persistent agent that calls a model, runs its requested tools and remembers the conversation
  - [Core](docs/agents/harnesses/more/mini/core/README.md): The core owns the repeated conversation between a model and its tools
  - [Runtime](docs/agents/harnesses/more/mini/runtime/README.md): The runtime makes a conversation survive the end of one request
  - [Link Adapter](docs/agents/harnesses/more/mini/link/README.md): Mini's Link adapter connects the small runtime to an installed agent
  - [Extend Mini](docs/agents/harnesses/more/mini/extensions.md): Mini's core and runtime can be reused with different model, tool and recording operations

### More

- [Decisions](docs/agents/harnesses/more/mini/decisions.md): Mini is the smallest persistent, tool-using harness built on Link's model and tool interfaces
- [Security](docs/agents/harnesses/more/mini/security.md): Mini relies on Link's harness hub, model interface, tool bridge and sandbox for authentication and confinement

<!-- docs-tree:end -->

## Code

- [`core/`](src/core/mod.rs): `run_turn`, the provider-independent model-and-tools loop
- [`link/`](src/link/mod.rs): adapter commands, Agent Client Protocol (ACP), model and tool connections
- [`runtime/`](src/runtime/mod.rs): complete conversation storage and optional instructions
- [`tests/`](tests/core/mod.rs): core, runtime and Link checks mirroring the source folders
- [`tool-turn.json`](tests/fixtures/tool-turn.json): the language-independent worked example executed by the core tests
