# Hermes Agent for Link

The Hermes Agent adapter, `lnk-hermes`, lets Local Link install,
sandbox, and run Hermes Agent.

<!-- docs-tree:start -->

## Docs

### Pages

- [Hermes Agent](docs/agents/harnesses/hermes/README.md): Hermes Agent is Nous Research's agent
  - [Hermes Agent Guide](docs/agents/harnesses/hermes/guide.md): Run Hermes Agent with a hosted or local model, and carry your conversations and memory between harnesses
  - [Runtime](docs/agents/harnesses/hermes/more/runtime.md): The hermes adapter installs, configures, and runs the native harness for Link

### More

- [Developing](docs/agents/harnesses/hermes/more/developing.md): Build and inspect the hermes adapter from its workspace package
- [Decisions](docs/agents/harnesses/hermes/decisions.md): Why `lnk-hermes` runs Hermes Agent the way it does
- [Security](docs/agents/harnesses/hermes/security.md): What `lnk-hermes` installs and runs, beyond what Link's sandbox guarantees every harness
- [Troubleshooting](docs/agents/harnesses/hermes/troubleshooting.md): What to do when Hermes Agent doesn't install, start or remember

<!-- docs-tree:end -->

## Code

- [`externals.json`](externals.json): the pinned release
- [`identity.rs`](src/identity.rs): who the agent is, and what it remembers, into Hermes and out
- [`main.rs`](src/main.rs): the adapter's commands
