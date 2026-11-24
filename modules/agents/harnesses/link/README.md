# Link Harness

Link Harness, `lnk-harness`, is Local Link's own harness, run like any other. It is a small router that answers the Agent Client Protocol (ACP), calls the model through Local Link's proxy, and keeps the conversation as events in the agent's files.

<!-- docs-tree:start -->

## Docs

### Pages

- [Link Harness](docs/agents/link/README.md): Link Harness is Local Link's own agent runner
  - [Link Harness Guide](docs/agents/link/guide.md): Configure Link Harness’s instructions, skills, tools, and model fallback, then inspect the prompt and conversation it keeps
  - [Agent Client Protocol](docs/agents/link/more/acp/README.md): Link Harness speaks ACP, so an ACP client can open a session on one of its conversations, prompt it, and receive the answer as it's written, with the tool calls beside it
  - [Commands](docs/agents/link/commands/README.md): A command is an owner's slash message that Link Harness handles
  - [Images, Documents and Voice Notes](docs/agents/link/media/README.md): Link Harness reads the images, documents and voice notes a message holds, from the Agent Client Protocol (ACP), or from any of Local Link's connections, and keeps each as a file in the conversation's folder
  - [Conversations](docs/agents/link/conversations.md): Link Harness retains conversations in your agent’s files or through its storage driver
  - [Harness Benchmarks](docs/agents/link/benchmarks.md): A controlled snapshot compares Link Harness's startup and idle memory with Hermes and OpenClaw

### More

- [Developing](docs/agents/link/more/developing.md): `lnk-harness` is Link Harness, the harness a Link agent runs by default: a small router in Rust that answers you, keeps the thread, and calls the tools Link runs for it
- [Decisions](docs/agents/link/more/decisions.md): Why Link Harness works the way it does
- [Troubleshooting](docs/agents/link/troubleshooting.md): What to do when Link Harness doesn't answer, or answers wrong

<!-- docs-tree:end -->

- [Decisions](docs/agents/link/commands/decisions.md): Why Link Harness's commands work the way they do
- [Decisions](docs/agents/link/media/decisions.md): Why Link Harness reads images and documents the way it does

## Code

- [`acp.rs`](src/acp.rs): ACP, where every client reaches it
- [`main.rs`](src/main.rs): the adapter and `serve`
- [`model.rs`](src/model.rs): calling the model
- [`store.rs`](src/store.rs): what the harness reads and writes
- [`turn.rs`](src/turn.rs): one turn
