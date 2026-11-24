# File Store

`lnk-file-store` is the driver of the `file` place. The driver keeps an agent's collections in the agent's folder on this machine, as ordinary files that a person reads and edits.

<!-- docs-tree:start -->

## Docs

- [File Store](docs/agents/more/stores/file.md): `lnk-file-store` is the `file` place's driver: an agent's collections in its folder on this machine (`LNK_STORE_DIR`), as a person reads and edits them (`link_store_contract::store::file`)

<!-- docs-tree:end -->

## Code

- [`main.rs`](src/main.rs): the driver
- [`module.toml`](module.toml): the module's manifest
