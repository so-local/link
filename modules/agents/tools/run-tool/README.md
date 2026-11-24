# Link Run

Link Run (`lnk-run-tool`) lets an agent run programs in its files folder, each call in a sandbox of its own, and keeps background commands running past the turn.

<!-- docs-tree:start -->

## Docs

### Pages

- [Run](docs/agents/tools/run/README.md): Your agent runs shell commands in the agent's files folder, each in a sandbox of its own
  - [Run Guide](docs/agents/tools/run/guide.md): Enable Run to execute shell commands in your agent’s files folder
  - [Run Scripts](docs/agents/tools/run/scripts.md): Give your agent Python or JavaScript, the two languages models write best, and it edits many files at once, works out sums and statistics, and queries tables, in a sandbox that reaches only what you grant

### Packages

- [Run Tool Contract](contract/README.md): `link-run-tool-contract` holds Link Run's command line, which the agent module builds and `lnk-run-tool` reads

### More

- [Developing](docs/agents/tools/run/more/developing.md): `lnk-run-tool` is Link Run: an MCP server over stdio that runs an agent's shell commands, each call in a sandbox of its own
- [Decisions](docs/agents/tools/run/more/decisions.md): Why each `run` call gets a sandbox of its own, which programs it offers, and how a command runs in the background
- [Troubleshooting](docs/agents/tools/run/troubleshooting.md): What to do when your agent can't run a program, reach a host, or keep a command running

<!-- docs-tree:end -->

## Code

- [`background.rs`](src/background.rs): background commands
- [`lib.rs`](src/lib.rs): the MCP server and `command`
- [`main.rs`](src/main.rs): the program
