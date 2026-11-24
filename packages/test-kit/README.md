# Testing Kit

`link-test-kit` holds the temp folders, programs, processes, and releases that tests need, written once so no test makes its own.

<!-- docs-tree:start -->

## Docs

- [Testing Kit](docs/more/developing/testing/testing-kit.md): `link-test-kit` holds what keeps a test free of races and of the machine it runs on, written once
- [Test Environments](docs/more/developing/testing/test-environments.md): Tests run on machines that differ in their user, kernel, paths, network and load

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): what the kit gives a test
- [`programs.rs`](src/programs.rs): the programs a test runs
- [`wait.rs`](src/wait.rs): waiting, the one way
