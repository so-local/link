# Storage Contract

> **Package**: [a library](../../../../docs/more/architecture/guide.md#kinds) that programs link, which runs nothing and owns no command

The package `link-storage-contract` defines what the storage module asks
of the modules built on it. Each ask is a question those modules answer:
which folders here are an owner's, which owned folder a cloud account
keeps pushed or pulled, and what an owner's ID is by its name.

<!-- docs-tree:start -->

## Docs

- [Storage Contract](docs/services/storage/more/contract.md): `link-storage-contract` holds the three questions `lnk storage` asks the modules built on it, as Link's answers carry them (`link_core::answers`): which folders here are an owner's, not yours, an owned folder a cloud account keeps pushed…

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): the questions and their answers
