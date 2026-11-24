# Sandbox Contract

The package `link-sandbox-contract` holds the sandbox's policy,
independent of how any sandbox implements it. The package reads a
sandbox's specification, resolves its permissions, and checks addresses and names
on the network, with no parser, backend, or proxy, for every side that
edits or enforces a policy. It also holds each command's runtime
arguments, with the line that gives them, so the program that reads a
line and a caller that writes one share one declaration.

<!-- docs-tree:start -->

## Docs

### Packages

- [Route Contract](route/README.md): The package `link-sandbox-route` defines a route, a way out of a sandbox beside the network that the sandbox's proxy mounts under `/_link/` and a bridge outside the sandbox serves
- [Sandbox Client](client/README.md): The package `link-sandbox-client` holds what a module that confines a program, such as `lnk-agent` for every harness, asks of `lnk sandbox`

### More

- [Sandbox Contract](docs/security/sandbox/more/contract.md): `link-sandbox-contract`: the sandbox's policy, independent of its implementation

<!-- docs-tree:end -->

## Code

- [`checks.rs`](src/checks.rs): the checks `actions.toml` names, which of a command's arguments go together
- [`net.rs`](src/net.rs): the network's addresses and names, as every way out checks them
- [`spec.rs`](src/spec.rs): a sandbox's specification, and each command's runtime arguments with the line that gives them
- [`system.rs`](src/system.rs): the baseline files exposed
