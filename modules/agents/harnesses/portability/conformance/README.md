# Harness Conformance

`link-harness-conformance` is the kit that checks a harness program answers the harness commands, by running the program as `lnk-agent` would. Every harness's tests run the kit, and a connection's tests use its agent on Mini Harness.

<!-- docs-tree:start -->

## Docs

- [Conformance Kit](docs/agents/harnesses/more/protocol/conformance.md): `link-harness-conformance` checks that a harness program answers the contract, by running it as `lnk-agent` would, without Link
- [Runtime Comparisons](docs/agents/harnesses/more/protocol/runtime.md): Compare Mini and full Link using built programs, local first-party model and connection services, and Rust records shared with Measure

<!-- docs-tree:end -->

## Code

- [`agent.rs`](src/agent.rs): an agent on the mini harness
- [`lib.rs`](src/lib.rs): the conformance checks
