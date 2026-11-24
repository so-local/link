# Sandbox Client

The package `link-sandbox-client` holds what a module that confines a
program, such as `lnk-agent` for every harness, asks of `lnk sandbox`.
The package carries the confinement a specification resolves to, a specification's keys as
flags, and the sandbox's own routes.

<!-- docs-tree:start -->

## Docs

- [Interface](docs/security/sandbox/more/contract/client.md): What other programs can rely on from `lnk sandbox`

<!-- docs-tree:end -->

## Code

- [`sandbox.rs`](src/sandbox.rs): the contract with `lnk-sandbox`
- [`sandbox/confinement.rs`](src/sandbox/confinement.rs): a confinement
- [`sandbox/routes.rs`](src/sandbox/routes.rs): the sandbox's own routes
- [`wires.rs`](src/wires.rs): current wire state and published traffic paths
