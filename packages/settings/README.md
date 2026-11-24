# Settings

`link-settings` holds Local Link's folder layout and application settings, so commands share them without depending on modules.

<!-- docs-tree:start -->

## Docs

### Pages

- [Settings](docs/more/libraries/files/settings.md): `link-settings` owns Home's locator, folder moves and application settings, so commands share their saved output settings without depending on modules

### More

- [Decisions](docs/more/libraries/files/settings/decisions.md): Application settings are shared by core and modules without depending on the module protocol
- [Settings Security](docs/more/libraries/files/settings/security.md): Folder relocation treats harness files as writable data and keeps every rewrite inside the directories it opened

<!-- docs-tree:end -->

## Code

- [`src/execution.rs`](src/execution.rs): Protection and staging cleanup before a run.
- [`src/lib.rs`](src/lib.rs): The library.
