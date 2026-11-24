# API Client

`link-api` is Local Link's typed client for the account service's API, with one function per route, built on `link-fetch`. Its types are made at every build from the API's specification, and it also checks and signs access tokens and installation assertions, with every credential held as a secret.

<!-- docs-tree:start -->

## Docs

- [API Client](docs/more/libraries/runtime/api-client.md): `link-api` is Link's typed client for the account service's API, one function a route, on `link-fetch`

<!-- docs-tree:end -->

## Code

- [`build.rs`](build.rs): the types, made from the specification
- [`jwt.rs`](src/jwt.rs): access tokens and installation assertions
- [`lib.rs`](src/lib.rs): the client, a function a route
