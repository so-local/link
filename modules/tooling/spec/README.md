# Specification Module

The specification module, `lnk spec`, lists, shows and converts the specification kinds that
every installed module exports. It runs no code from the module that owns
each kind.

<!-- docs-tree:start -->

## Docs

### Pages

- [Specification Kinds](docs/config/specs/guides/kinds/README.md): `lnk spec` lists every specification kind, shows one's fields and rewrites a file in its current schema

### References

- [Specification Commands](docs/more/reference/modules/tooling/spec.md): Every command the `spec` module offers, from its declarations, in order of the resource each acts on

<!-- docs-tree:end -->

## Code

- [`cli.rs`](src/cli.rs): its command line and outputs, generated from `actions.toml` and `types.toml`
- [`kinds.rs`](src/kinds.rs): the kinds the installed modules export
- [`main.rs`](src/main.rs): its commands
