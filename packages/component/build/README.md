# Component Build

`link-component-build` is what a package's build script calls to embed its component description and the pages it owns. It reads the description from Cargo and the package's declarations, and discovery never runs an installer or resolves Cargo.

<!-- docs-tree:start -->

## Docs

- [Component Build](docs/more/libraries/types/component-metadata/build.md): `link-component-build` is what a package's build script calls to embed its component description, read from Cargo and its declarations, and the pages it owns

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): embedding a package's declarations
