# Machine

`link-machine` knows this installation, the machine it runs on, and whose it is. It holds the installation's ID and name, the machine's kind, the boxes it knows, and the rules that every name, ID, and owner Local Link makes must follow.

<!-- docs-tree:start -->

## Docs

### Pages

- [Installations](docs/config/installation.md): An installation is Link under one OS user; the machine it runs on is hardware

### More

- [Decisions](docs/config/installation/decisions.md): Installation identity has one owner, independent of the modules that use it

<!-- docs-tree:end -->

## Code

- [`apt.rs`](src/apt.rs): how apt retries a slow mirror on a machine Link sets up
- [`installation.rs`](src/installation.rs): this installation's ID and name
- [`machine.rs`](src/machine.rs): the machine's kind
- [`names.rs`](src/names.rs): the rules for names and IDs
- [`owner.rs`](src/owner.rs): whose a machine's work is
