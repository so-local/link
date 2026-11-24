# Checks and Timings

Checks and Timings runs a workspace's checks as one command, `lnk-check`, and turns its test run into a timing report that sorts each failure by whose it is.

<!-- docs-tree:start -->

## Docs

- [Checks and Timings](docs/more/developing/testing/test-timing.md): `lnk-check` runs a workspace's checks as one command, and its report shows where the test time goes and whose each failure is

<!-- docs-tree:end -->

## Code

- [`src/check_main.rs`](src/check_main.rs): `lnk-check`, the checks as one command
- [`src/checks.rs`](src/checks.rs): the steps `.cargo/checks.toml` declares, and running them
- [`src/lib.rs`](src/lib.rs): the saved measurement and ownership grouping
- [`src/main.rs`](src/main.rs): the offline report command
- [`src/render.rs`](src/render.rs): the standalone chart and tables
