# DeepSeek Harness for Link

The DeepSeek Harness adapter, `lnk-deepseek`, lets Local Link install,
sandbox, and run DeepSeek Harness (`dsh`). It serves the harness's Agent
Client Protocol (ACP) to Link's connections through the harness hub.

<!-- docs-tree:start -->

## Docs

### Pages

- [DeepSeek Harness](docs/agents/harnesses/deepseek/README.md): DeepSeek Harness is DeepSeek's open-source agent
  - [DeepSeek Harness Guide](docs/agents/harnesses/deepseek/guide.md): Run DeepSeek Harness with your chosen provider or local runtime, then connect it to your chats
  - [Runtime](docs/agents/harnesses/deepseek/more/runtime.md): The deepseek adapter installs, configures, and runs the native harness for Link

### More

- [Developing](docs/agents/harnesses/deepseek/more/developing.md): Build and inspect the deepseek adapter from its workspace package
- [Decisions](docs/agents/harnesses/deepseek/decisions.md): Why `lnk-deepseek` runs DeepSeek Harness the way it does
- [Security](docs/agents/harnesses/deepseek/security.md): What `lnk-deepseek` installs and runs, beyond what Link's sandbox guarantees every harness
- [Troubleshooting](docs/agents/harnesses/deepseek/troubleshooting.md): What to do when DeepSeek Harness doesn't install, start or answer

<!-- docs-tree:end -->

## Code

- [`externals.json`](externals.json): the pinned release
- [`main.rs`](src/main.rs): the adapter's commands
