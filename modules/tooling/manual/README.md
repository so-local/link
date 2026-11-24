# Manual Module

The manual module, `lnk man`, reads the documentation and the command
reference of installed components offline. It reads them from the core's
registry.

<!-- docs-tree:start -->

## Docs

### Pages

- [Manual](docs/tooling/manual/README.md): `lnk man` browses the documentation tree, offline

### References

- [Manual Commands](docs/more/reference/modules/tooling/manual.md): Every command the `manual` module offers, from its declarations, in order of the resource each acts on

<!-- docs-tree:end -->

## Code

- [`cli.rs`](src/cli.rs): command parser shared with documentation checks
- [`main.rs`](src/main.rs): offline manual execution
