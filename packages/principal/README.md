# Principal

`link-principal` names who acts, or may act, as one type, `user:<id>` or `service:<id>`. An access token's subject, a role's holder, and a teammate's sender all use that type, so they can't disagree.

<!-- docs-tree:start -->

## Docs

### Pages

- [Principal](docs/more/libraries/types/principal.md): `link-principal` is who acts, or may, as one type: `user:<id>` or `service:<id>`, as the account service's API names it

### More

- [Principal Decisions](docs/more/libraries/types/principal/decisions.md): Why a principal is a user or a service and nothing else

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): the type and its grammar
