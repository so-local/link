# Token

`link-token`: Link's one access token, a short signed JWT saying who acts
and from which kind of client, never what's allowed, and the long-lived
values traded for one.

<!-- docs-tree:start -->

## Docs

- [Token](docs/more/libraries/files/token.md): `link-token` is Link's one access token: a short signed JWT (JSON Web Token) saying who acts and from which kind of client, never what's allowed

<!-- docs-tree:end -->

## Code

- [`limits.rs`](src/limits.rs): the limits, from `limits.toml`
- [`prefixes.rs`](src/prefixes.rs): each type's prefix, from `prefixes.toml`
- [`refresh.rs`](src/refresh.rs): refresh tokens
- [`secret.rs`](src/secret.rs): random secrets with a checksum
- [`token.rs`](src/token.rs): issuing and verifying an access token
