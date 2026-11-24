# Bucket Store

`link-bucket-store` is the driver of the `bucket` place
(`lnk-bucket-store`). The driver keeps an agent's collections as objects in
the agent's encrypted bucket, the one `lnk storage` connected for the agent,
through the engine of `lnk storage`.

<!-- docs-tree:start -->

## Docs

- [Bucket Store](docs/agents/more/stores/bucket.md): `lnk-bucket-store` is the `bucket` place's driver: an agent's collections as objects in its bucket, the first encrypted one `lnk storage` connected for it, through `lnk storage`'s engine

<!-- docs-tree:end -->

## Code

- [`main.rs`](src/main.rs): the driver's commands
- [`store.rs`](src/store.rs): the collections as objects in the bucket
