# Secretive Derive

`link-secretive-derive` provides `#[derive(Duplicate)]` for a type that holds a secret. Such a type has no `Clone`, so code copies it only by saying so. Each field uses `Clone`, and a field holding a secret uses its own `duplicate()`.

<!-- docs-tree:start -->

## Docs

- [Secretive Derive](docs/more/libraries/files/secretive/derive.md): `link-secretive-derive` is `#[derive(Duplicate)]`, for a type that holds a secret

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): the derive
