# Specifications

`link-spec` reads specifications strictly, as typed TOML, and writes the paths in them so a specification means the same on any machine. It refuses an unknown key and never ignores one.

<!-- docs-tree:start -->

## Docs

- [Specifications](docs/more/libraries/types/specifications.md): `link-spec` reads specifications strictly, as typed TOML, and the paths in them, written so a specification means the same on any machine
- [Specifications](docs/more/reference/specs.md): Write repeatable configuration in TOML and supply it to a command as a file or field flags

<!-- docs-tree:end -->

## Code

- [`fields.rs`](src/fields.rs): every field's flag, made from the kind's shape
- [`lib.rs`](src/lib.rs): reading a specification, and its paths
