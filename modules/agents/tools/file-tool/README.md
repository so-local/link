# Link File

Link File (`lnk-file-tool`) lets an agent `read`, `write`, `edit`, `glob`, and `grep` its files, each call in a sandbox of its own.

<!-- docs-tree:start -->

## Docs

### Pages

- [Files](docs/agents/tools/file/README.md): Your agent reads, writes, and changes the files in the agent's files folder, as the agent's sandbox file allows
  - [Files Guide](docs/agents/tools/file/guide.md): Enable File to read, write, edit, and find files within your agent’s granted folders

### Packages

- [File Tool Contract](contract/README.md): `link-file-tool-contract` holds Link File's command line, which the agent module and Local Link's units build and `lnk-file-tool` reads

### More

- [Developing](docs/agents/tools/file/more/developing.md): `lnk-file-tool` is Link File: an MCP server over stdio that reads and writes an agent's files, its calls in a sandbox
- [Decisions](docs/agents/tools/file/more/decisions.md): Why `file`'s calls are Claude Code's, why it keeps to `[files]` before its sandbox does, what it narrows per call, and how it works through a files bridge and in a bucket
- [Troubleshooting](docs/agents/tools/file/troubleshooting.md): What to do when your agent can't write or read one of its files

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): the MCP server and its tools
- [`main.rs`](src/main.rs): the program
