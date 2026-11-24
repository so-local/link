# Types

> **Package**: [a library](../../docs/more/architecture/guide.md#kinds) that programs link, which runs nothing and owns no command

`link-types` holds the shapes every module shares, such as an OS, a processor, a box's user, and a duration, with the report and MCP answers an action may give. Local Link generates them from core's declarations.

<!-- docs-tree:start -->

## Docs

- [Shared Types](docs/more/developing/modules/catalog/shared-types.md): `link-types` holds core's shapes, the ones every module shares, generated once from core's declarations so each has one home and one Rust type

<!-- docs-tree:end -->

## Code

- [`arch.rs`](src/arch.rs): a processor's word and default
- [`lib.rs`](src/lib.rs): the generated shapes, and a duration's length
- [`os.rs`](src/os.rs): an OS's word and default
- [`user.rs`](src/user.rs): root or a user of the box by name
