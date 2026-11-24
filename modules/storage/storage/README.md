# Storage Module

> **Module**: [a program](../../../docs/more/architecture/guide.md#kinds), `lnk-<name>`, that `lnk` runs and others reach only through its commands

The storage module (`lnk-storage`) keeps your files (`~/Link/Files`) and
each owner's folder, one another module says is its own. The module backs
up each to its own bucket in every cloud you connect and checks them,
with rclone as the engine.

<!-- docs-tree:start -->

## Docs

### Pages

- [Storage](docs/services/storage/README.md): Storage backs up your folders to the clouds you connect, verifies each copy, and restores files when you need them
  - [Storage Guide](docs/services/storage/guide.md): Your folders, in your clouds
  - [Files](docs/services/storage/files.md): The files `lnk storage` keeps in `~/.lnk/safe/storage/`, and what each one holds

### References

- [Storage Commands](docs/more/reference/modules/storage/storage.md): Every command the `storage` module offers, from its declarations, in order of the resource each acts on

### Packages

- [Storage Contract](contract/README.md): The package `link-storage-contract` defines what the storage module asks of the modules built on it

### More

- [Interface](docs/services/storage/more/protocol.md): What other programs can rely on from `lnk storage`
- [Decisions](docs/services/storage/decisions.md): Why buckets work the way they do
- [Security](docs/services/storage/security.md): What buckets protect: your files and their copies in your own clouds, against losing the last good copy, and the keys to those clouds
- [Troubleshooting](docs/services/storage/troubleshooting.md): What to do when [a `lnk storage` command](README.md) refuses or misses files

<!-- docs-tree:end -->

## Code

- [`cli.rs`](src/cli.rs): its command line and outputs, generated from `actions.toml` and `types.toml`, with its check hooks in `checks.rs`
- [`engine.rs`](src/engine.rs): what the engine must do
- [`main.rs`](src/main.rs): the `lnk storage` commands
- [`ops.rs`](src/ops.rs): what each command does
- [`rclone.rs`](src/rclone.rs): rclone, the engine
