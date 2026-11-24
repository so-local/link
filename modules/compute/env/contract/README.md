# Environment Contract

`link-env-contract` is the package that reads the environment file as
strictly as every specification is read. It also holds the file's stages, and the
requests and process clients other modules use to reach an environment.
It gives a run the secrets its file was granted, and it answers which file
a grant is for. The `lnk-env` module runs the environment.

<!-- docs-tree:start -->

## Docs

### Pages

- [Environment Files](docs/services/environments/files.md): An environment file says what every machine made from it has before work starts

### More

- [Environment Contract](docs/services/environments/more/contract.md): `link-env-contract` is the environment file, read strictly as every specification is, its stages, and the wire requests and process clients other modules reach an environment through

<!-- docs-tree:end -->

## Code

- [`file.rs`](src/file.rs): the environment file
- [`remote.rs`](src/remote.rs): an environment on a box
- [`stage.rs`](src/stage.rs): a stage, as every side runs one
