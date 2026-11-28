# Agent Binding

The package `link-sandbox-binding` keeps the agents bound on this machine, and their credentials, outside every sandbox. Every bridge checks a call's proof against them. Binding writes an agent's credential, and unbinding revokes it. The package also carries `link-sandbox-bridge` as every bridge shares it.

<!-- docs-tree:start -->

## Docs

- [Agent Binding](docs/tooling/sandbox/more/bridges/binding.md): `link-sandbox-binding`: the agents bound on this machine and their keys, kept outside every sandbox, which every bridge checks a call's proof against

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): the keyring and its bindings
