# Store Contract

`link-store-contract` defines where an agent's things are kept: the six collections and their places, the agreement between a harness and a store driver, the files bridge over a bucket, and the storage credential that refreshes itself.

<!-- docs-tree:start -->

## Docs

### Pages

- [Files Bridge](docs/agents/more/stores/files-bridge.md): How an agent's Home kept in a bucket is reached from its sandbox, and the storage credential the bridge holds, both in `link-store-contract`

### More

- [Interface](docs/agents/more/stores/protocol.md): The commands a store driver, `lnk-<name>`, answers for an agent's collections

<!-- docs-tree:end -->

## Code

- [`authority.rs`](src/authority.rs): a refreshing storage credential
- [`files.rs`](src/files.rs): the files bridge
- [`storage.rs`](src/storage.rs): collections and their places
- [`store.rs`](src/store.rs): the store driver contract
