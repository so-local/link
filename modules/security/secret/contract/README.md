# Secrets Contract

The package `link-secret-contract` defines the secrets a person keeps on
this machine, as every program that gives one reads them. Each value is
in the Keychain where there is one, and otherwise in Local Link's
settings, with how it's given and where it's granted beside it.

A secret goes only where a grant names it. A sandboxed program never
holds one, because the secret route adds it to a call outside the
sandbox. The command `lnk-secret` is the command line for secrets.

<!-- docs-tree:start -->

## Docs

- [Secrets Contract](docs/config/secrets/more/contract.md): `link-secret-contract`, the library every program that keeps or gives a secret reads it through: the store, its grants, the keeper's protocol and the secret route's types

<!-- docs-tree:end -->

## Code

- [`secrets.rs`](src/secrets.rs): the store
- [`secrets/keeper.rs`](src/secrets/keeper.rs): who holds secrets
- [`secrets/route.rs`](src/secrets/route.rs): the secret route
