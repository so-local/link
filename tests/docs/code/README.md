# Docs Code Checks

`link-docs-check-code` checks documentation examples and inventories against the command parsers, types, and declarations Local Link releases.

<!-- docs-tree:start -->

## Docs

- [Docs Code Checks](docs/more/developing/documentation/checks.md): `link-docs-check-code` checks documentation against the code Link releases

<!-- docs-tree:end -->

## Code

- [`lnk-docs-inventories.rs`](src/bin/lnk-docs-inventories.rs): inventory and reference writer
- [`docs_code.rs`](tests/docs_code.rs): the registered rules and integration test
- [`inventories.rs`](src/inventories.rs): source-backed inventories and freshness checks
- [`parsers.rs`](src/parsers.rs): actual command parsers shared with contract tests
- [`references.rs`](src/references.rs): actual specification field and default discovery
