# Components

`link-component` describes a component in typed form. A component is a program `lnk` installs and runs on its own, such as a module, a harness program, or a store driver, and its build, the installed program, and `lnk` all share that description.

<!-- docs-tree:start -->

## Docs

### Pages

- [Components](docs/more/libraries/types/component-metadata.md): A component is a program `lnk` installs and runs on its own: a module, a harness program, a connection, a tool, a store driver or a standalone program

### Packages

- [Component Build](build/README.md): `link-component-build` is what a package's build script calls to embed its component description and the pages it owns

<!-- docs-tree:end -->

## Code

- [`cargo.rs`](src/cargo.rs): its identity, read from Cargo
- [`lib.rs`](src/lib.rs): a component's identity, its `module.toml` and the answer an installed program gives
- [`requirements.rs`](src/requirements.rs): what one operation needs installed
