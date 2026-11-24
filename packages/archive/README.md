# Archive

`link-archive` carries a thing's state as one `.tar.gz` file. It reads and writes archives by files alone, the same way for every kind of thing.

<!-- docs-tree:start -->

## Docs

- [Archive Package](docs/tooling/archive/package.md): `link-archive` reads and writes archives for every kind, knowing keys, owners and tables, never what an agent is

<!-- docs-tree:end -->

## Code

- [`src/export.rs`](src/export.rs): writing the things selected to one archive
- [`src/import.rs`](src/import.rs): naming, placing and adopting each thing imported
- [`src/lib.rs`](src/lib.rs): the package, its modules and this machine's roots
- [`src/reading.rs`](src/reading.rs): the strict reader and staging
- [`src/writing.rs`](src/writing.rs): the writer
