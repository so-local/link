# Stores

Stores hold the collections an agent keeps, through one store contract and a driver for each place.

<!-- docs-tree:start -->

## Docs

### Pages

- [Stores](docs/agents/more/stores/README.md): Everything your agent keeps is kept at a place: the agent's folder on this machine, a SQLite file, or an encrypted bucket of the agent's own

### Modules

- [Bucket Store](bucket-store/README.md): `link-bucket-store` is the driver of the `bucket` place (`lnk-bucket-store`)
- [File Store](file-store/README.md): `lnk-file-store` is the driver of the `file` place
- [SQLite Store](sqlite-store/README.md): The store driver `lnk-sqlite-store` keeps an agent’s collections except Home in one SQLite file that moves with the agent's data

### Packages

- [Store Contract](contract/README.md): `link-store-contract` defines where an agent's things are kept: the six collections and their places, the agreement between a harness and a store driver, the files bridge over a bucket, and the storage credential that refreshes itself

<!-- docs-tree:end -->
