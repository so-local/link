# Secret Module

The secret module, `lnk-secret`, keeps your secrets on this machine and
gives them only where you grant them.

<!-- docs-tree:start -->

## Docs

### Pages

- [Secrets](docs/config/secrets/README.md): A secret is any value Local Link keeps private for you, such as an API key, a token or a password
  - [Environment Secrets](docs/config/secrets/environment.md): Give an API key to an environment you run without putting its value in the environment file
  - [Secret Keeper](docs/config/secrets/keeper/README.md): The keeper is a system user that keeps your secrets, and no program of yours runs as that user

### References

- [Secret Commands](docs/more/reference/modules/security/secret.md): Every command the `secret` module offers, from its declarations, in order of the resource each acts on

### Packages

- [Secrets Contract](contract/README.md): The package `link-secret-contract` defines the secrets a person keeps on this machine, as every program that gives one reads them

### More

- [Interface](docs/config/secrets/more/protocol.md): How a program is handed the secrets it may use, never through its environment
- [Decisions](docs/config/secrets/decisions.md): Why `lnk secret` works the way it does
- [Troubleshooting](docs/config/secrets/troubleshooting.md): What to do when [`lnk secret`](README.md) refuses a grant, leaves a copy behind, or can't reach its keeper

<!-- docs-tree:end -->

## Code

- [`bridge.rs`](src/bridge.rs): the secret route's bridge
- [`keeper.rs`](src/keeper.rs): the Linux keeper
- [`main.rs`](src/main.rs): `lnk secret`'s commands
