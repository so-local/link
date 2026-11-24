# Stable IDs

`link-id` holds stable identities, written `<kind>:<id>`, as one generic type that every kind of ID uses. It names a workload such as `agent:<its ID>` and one run of it such as `agent:<its ID>@<run>`, with its parent, and it draws new IDs under rules their owners reserve and retire them by.

<!-- docs-tree:start -->

## Docs

### Pages

- [Stable IDs](docs/more/libraries/types/id.md): `link-id` names a thing by what never changes, `<kind>:<id>`, with one generic every kind of ID rides

### More

- [Stable ID Decisions](docs/more/libraries/types/id/decisions.md): Why Link names things by minted IDs, and why every ID keeps one form wherever it's written

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): the generic, workloads and executions
- [`limits.toml`](src/limits.toml): candidates' shape, reserving's attempts and parents' depth
- [`reserving.rs`](src/reserving.rs): the reservation contract and its table
- [`tests.rs`](src/tests.rs): their grammar and how they're written
