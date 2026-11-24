# Version

`link-version` is a version of `lnk`, ordered as semantic versions are. Every place that reads, compares, or carries a version uses this one type, so callers share its validation and ordering rules.

<!-- docs-tree:start -->

## Docs

- [Version](docs/more/libraries/types/version.md): `link-version` is a version of `lnk`, as semantic versioning writes and orders them: the one type every place that reads, compares or carries one uses

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): the type and its order
- [`orders.txt`](orders.txt): version pairs in order, which the tests hold to
- [`protocol.rs`](src/protocol.rs): a protocol's version, as its owner declares it
