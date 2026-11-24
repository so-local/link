# Agent State Protocol

`link-agent-state-protocol` implements the Agent State Protocol (ASP). ASP carries an agent's state between harnesses as extension methods on the Agent Client Protocol (ACP), through the protocol's own Rust SDK.

<!-- docs-tree:start -->

## Docs

- [Agent State Protocol](docs/agents/harnesses/more/agent-state-protocol.md): The Agent State Protocol (ASP) is Link's extension to the Agent Client Protocol (ACP) for carrying an agent's state between harnesses, so a conversation can continue after changing the program that runs it

<!-- docs-tree:end -->

## Code

- [`forms.rs`](src/forms.rs): each kind of state in its portable form
- [`lib.rs`](src/lib.rs): the agent-state capability, under `_meta`
- [`methods.rs`](src/methods.rs): `_asp/state/describe`, `export` and `import`
- [`steering.rs`](src/steering.rs): what a prompt sent while a turn runs does
