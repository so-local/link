# Actions Build

> **Package**: [a library](../../../docs/more/architecture/guide.md#kinds) that programs link, which runs nothing and owns no command

`link-actions-build` writes a module's Rust from the catalog at build time. That Rust holds the validating types, specification kinds, typed arguments, command-line parser, handler trait, and dispatch.

<!-- docs-tree:start -->

## Docs

- [Generation](docs/more/developing/modules/catalog/generation.md): `link-actions-build` writes a module's Rust from the catalog at build time, so its types refuse what its declarations refuse, its command line is its declarations', and every surface calls its handler through one typed dispatch

<!-- docs-tree:end -->

## Code

- [`commands.rs`](src/commands.rs): each action's typed arguments and the command line
- [`lib.rs`](src/lib.rs): the build's one call and the parts it writes
- [`specs.rs`](src/specs.rs): each specification kind made a specification `link-spec` reads
- [`types.rs`](src/types.rs): each shape as a type that refuses what its shape refuses
