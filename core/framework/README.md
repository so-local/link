# Module Protocol

`link-core` is the Local Link package that describes, discovers, installs,
updates, and removes modules, and checks their signed releases.

<!-- docs-tree:start -->

## Docs

### Pages

- [Module Installation](docs/more/modules/installation/README.md): `link-core` is the package that installs, upgrades and removes modules for `lnk`, and checks each program before it runs
  - [Outside Module Releases](docs/more/modules/installation/outside.md): A module from outside Link comes from its maker's own signed release, never from Link's
  - [Signed Releases](docs/more/modules/installation/releases.md): How a release is signed, who checks it, and what keeps an older one from coming back

### More

- [Decisions](docs/more/modules/installation/decisions.md): Why `link-core` installs, declares and removes modules the way it does
- [Module Protocol](docs/more/modules/protocol.md): A module is a program installed and managed by `lnk`

<!-- docs-tree:end -->

## Code

- [`descriptor.rs`](src/descriptor.rs): executable descriptions
- [`lib.rs`](src/lib.rs): the protocol
- [`release.rs`](src/release.rs): verified downloads
