# Bridge

The package `link-sandbox-bridge` defines what every bridge out of an agent's sandbox is. A bridge is a program outside the sandbox that holds what the sandbox never sees. The package defines who calls a bridge, the caller's proof and its action, the server every bridge runs on, and the test every bridge passes.

<!-- docs-tree:start -->

## Docs

### Pages

- [Bridges](docs/tooling/sandbox/more/bridges.md): What every bridge out of an agent's sandbox speaks, through `link-sandbox-bridge`

### Packages

- [Agent Binding](binding/README.md): The package `link-sandbox-binding` keeps the agents bound on this machine, and their credentials, outside every sandbox

<!-- docs-tree:end -->

## Code

- [`call.rs`](src/call.rs): a caller's side of one call
- [`conformance.rs`](src/conformance.rs): the test every bridge passes
- [`lib.rs`](src/lib.rs): what a bridge and a call are
- [`serve.rs`](src/serve.rs): the server every bridge runs on
- [`take.rs`](src/take.rs): a bridge's side of one call
