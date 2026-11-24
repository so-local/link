# Declarations Fixture

`link-declarations-fixture` compiles a small module's declarations
through [Actions Build](../../../packages/actions/build/README.md), so
tests run the generated types, command line and dispatch end to end.

Its module is an environment's two actions, `lnk env create` and `lnk
env approval add`, with the `env` specification kind, a stage and their outputs.
Its tests parse command lines through the shared entry, read the specification
from a file and its field flags alike, and dispatch calls to a handler
keeping them in memory.

## Code

- [`generated.rs`](tests/generated.rs): the end-to-end tests
- [`lib.rs`](src/lib.rs): the generated module and its in-memory handler
