# Service Contract

`link-service` holds the shared background-service API, with native and explicitly selected Linux guest implementations. It also holds the bounded exchange one Local Link program uses to ask another.

<!-- docs-tree:start -->

## Docs

- [Service Contract](docs/more/libraries/runtime/services.md): `link-service` runs background services as `lnk`'s modules run them, through one job and lifecycle API
  - [Program Exchange](docs/more/libraries/runtime/services/exchange.md): One of Link's programs asks another for something by starting it, writing one JSON request to its standard input and reading one JSON reply from its standard output

<!-- docs-tree:end -->

## Code

- [`guest.rs`](src/guest.rs): explicit Linux guest selection and lifecycle
- [`protocol.rs`](src/protocol.rs): one bounded JSON exchange with a child program
- [`service.rs`](src/service.rs): installing and running a service
