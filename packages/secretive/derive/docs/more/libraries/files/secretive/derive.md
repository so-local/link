# Secretive Derive

`link-secretive-derive` is `#[derive(Duplicate)]`, for a type that
holds a secret. Such a type has no `Clone`, so a copy of it is always
said: its `duplicate()` copies each field, a field marked
`#[duplicate]` (one holding a secret) by its own `duplicate()`, every
other by `Clone`.

## Using it

The trait it implements, [`Duplicate`](../../../../../../docs/more/libraries/files/secretive.md), must be in scope where it's
derived: `use link_secretive::Duplicate;` brings both the trait and
the derive.
