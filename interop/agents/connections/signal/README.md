# Signal for Link

The Signal connection, `lnk-signal`, lets your agent answer you in Signal.
The agent's number is linked as a device through `signal-cli`, and
`lnk-signal bridge` carries the agent's messages for a harness that speaks no
Signal.

<!-- docs-tree:start -->

## Docs

### Pages

- [Signal](docs/agents/connections/signal/README.md): Text your agent on Signal
  - [Signal Guide](docs/agents/connections/signal/guide.md): Link a dedicated Signal number to your agent, then send messages from your own number
  - [Native Program](docs/agents/connections/signal/more/native-program.md): The Signal adapter installs and links `signal-cli` for the harness and its bridge

### More

- [Decisions](docs/agents/connections/signal/decisions.md): How the Signal module connects an agent, through `signal-cli`
- [Troubleshooting](docs/agents/connections/signal/troubleshooting.md): What to do when your agent's Signal number doesn't link or doesn't answer

<!-- docs-tree:end -->

## Code

- [`bridge.rs`](src/bridge.rs): linking and the bridge
- [`main.rs`](src/main.rs): `lnk-signal connect` and its commands
- [`native.rs`](src/native.rs): `signal-cli`, as the harness runs it
