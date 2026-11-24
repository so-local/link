# OpenClaw for Link

The OpenClaw adapter, `lnk-openclaw`, lets Local Link install, sandbox,
and run OpenClaw. The agent module, `lnk-agent`, runs it through the
commands every harness answers.

<!-- docs-tree:start -->

## Docs

### Pages

- [OpenClaw](docs/agents/harnesses/openclaw/README.md): OpenClaw is the personal AI assistant, with its own personality files and commands you send from your phone
  - [OpenClaw Guide](docs/agents/harnesses/openclaw/guide.md): Run OpenClaw with a hosted model or Ollama, then use its personality and memory from your existing chats
  - [Runtime](docs/agents/harnesses/openclaw/more/runtime.md): The openclaw adapter installs, configures, and runs the native harness for Link

### More

- [Developing](docs/agents/harnesses/openclaw/more/developing.md): Build and inspect the openclaw adapter from its workspace package
- [Decisions](docs/agents/harnesses/openclaw/decisions.md): Why `lnk-openclaw` runs OpenClaw the way it does
- [Security](docs/agents/harnesses/openclaw/security.md): What `lnk-openclaw` installs and runs, beyond what Link's sandbox guarantees every harness
- [Troubleshooting](docs/agents/harnesses/openclaw/troubleshooting.md): What to do when OpenClaw doesn't find its model, loses a turn or keeps starting outside Link

<!-- docs-tree:end -->

## Code

- [`externals.json`](externals.json): the OpenClaw release it installs
- [`main.rs`](src/main.rs): the adapter's commands
