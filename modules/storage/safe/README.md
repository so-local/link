# Safe

`link-safe` declares the Safe, where the control files that steer Local
Link live, and the actions that list, read, and write a Safe's files,
each named by the Safe's ID, `safe:<id>`. Every action is control, so no
run is ever given one.

<!-- docs-tree:start -->

## Docs

- [Safe](docs/config/safe/README.md): A Safe holds the control files that steer Local Link, such as a sandbox's grants, a module's settings, and Link Models' settings, and the `safe` module is how they're listed, read, and written

<!-- docs-tree:end -->

## Code

- [`declared.rs`](src/declared.rs): the shapes, generated from `types.toml`
- [`lib.rs`](src/lib.rs): the package
