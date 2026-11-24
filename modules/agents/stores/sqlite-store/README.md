# SQLite Store

The store driver `lnk-sqlite-store` keeps an agent’s collections except
Home in one SQLite file that moves with the agent's data. Conversations, narrowed
tools, scheduled jobs, and versioned files share that database.

<!-- docs-tree:start -->

## Docs

- [SQLite Store](docs/agents/more/stores/sqlite.md): `lnk-sqlite-store` is the `sqlite` place's driver: an agent's things as rows of one file on this machine, `events.sqlite` in the folder its caller names (`LNK_STORE_DIR`, the agent's `chats/`), so it moves with the agent's files

<!-- docs-tree:end -->

## Code

- [`main.rs`](src/main.rs): the driver
- [`module.toml`](module.toml): the module's manifest
