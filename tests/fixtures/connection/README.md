# Connection Fixture

`lnk-connection-fixture` demonstrates how a connection carries messages, as the [connection contract](../../../modules/agents/connections/contract/docs/agents/connections/more/protocol/README.md)
describes, against a test's fake bot service. Read it to follow pairing, allowed-user checks, polling,
delivery to a harness and sending the answer back. It is never shipped.

## Code

- [`actions.toml`](declared/actions.toml): every command of the connection protocol, listed under `[implements]`
- [`lib.rs`](src/lib.rs): its command line, generated from those declarations
- [`main.rs`](src/main.rs): the fake service protocol, connection description, pairing, and bridge
