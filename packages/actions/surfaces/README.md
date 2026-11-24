# Actions Surfaces

> **Package**: [a library](../../../docs/more/architecture/guide.md#kinds) that programs link, which runs nothing and owns no command

`link-actions-surfaces` writes every surface from the catalog. The surfaces are the OpenAPI document, TypeScript types and client, a page per action, agents' tools, the command, specification, and variable references, and shell completion.

<!-- docs-tree:start -->

## Docs

- [Surfaces](docs/more/developing/modules/catalog/surfaces.md): `link-actions-surfaces` writes every surface Link offers beside its command line from the catalog of every module's declarations, so the API, its typed client, agents' tools, and the references can never disagree about a command or a shape

<!-- docs-tree:end -->

## Code

- [`files.rs`](src/files.rs): what the generators write, and its freshness
- [`lib.rs`](src/lib.rs): the generators, one module per surface
- [`surfaces.toml`](surfaces.toml): where and how each surface is written
