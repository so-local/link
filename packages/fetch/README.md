# Fetch

`link-fetch` is an HTTP client with explicit transport policy, bounded responses, and consistent errors. Callers use the defaults or declare exceptions through the shared builder, then read typed answers or bounded byte streams.

<!-- docs-tree:start -->

## Docs

### Pages

- [Fetch](docs/more/libraries/runtime/fetch.md): `link-fetch` gives callers an HTTP client with bounded responses, explicit transport policy, typed answers, and consistent errors

### More

- [Fetch Decisions](docs/more/libraries/runtime/fetch/decisions.md): Why every call of Link's own goes out on one client, `link-fetch`, and why that client reads answers as it does

<!-- docs-tree:end -->

## Code

- [`blocking.rs`](src/blocking.rs): the same client, for code with no runtime
- [`lib.rs`](src/lib.rs): the client and its rules
