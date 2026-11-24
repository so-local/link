# Link Memory

Link Memory (`lnk-memory-tool`) runs locally and lets an agent search,
read, and list its folder with no credential. The tool indexes the
agent's `memory/` folder and conversations, and reads unindexed skill
files by their paths.

<!-- docs-tree:start -->

## Docs

### Pages

- [Memory](docs/agents/tools/memory/README.md): Your agent's memory is a search over the agent's own files
  - [Memory Guide](docs/agents/tools/memory/guide.md): Start Link Memory, add searchable documents, and choose whether searches match words or meaning
  - [Memory's Index](docs/agents/tools/memory/more/index.md): How Link Memory keeps each agent's index, writes it in the background, and keeps it whole through a crash, a schema change or a full disk
  - [How Memory Stays Current](docs/agents/tools/memory/more/freshness.md): How Link Memory learns that a file changed without walking the agent's folder to answer a call
  - [What Is Embedded, and When](docs/agents/tools/memory/more/embedding.md): How Link Memory chooses which passages to embed next, how one thread embeds for every agent a service serves, and how the vectors are kept

### Modules

- [Link Document Converter](document-converter/README.md): Link Document Converter (`lnk-document-converter`) is a module that reads the text of PDFs and Word documents for Link Memory's index

### Packages

- [Memory Contract](contract/README.md): `link-memory-tool-contract` holds the agreement between the agent module and a memory engine's adapter for `lnk agent memory start <engine>`, and how a bound engine proves itself at an embedding endpoint

### More

- [Developing](docs/agents/tools/memory/more/developing.md): `lnk-memory-tool` is Link Memory, the memory engine a Link agent uses by default: an MCP server over stdio that searches, reads and lists the agent's files and past conversations, keyless and local
- [Decisions](docs/agents/tools/memory/more/decisions.md): Why Link Memory is named, writes, indexes, is sandboxed and searches as it does
- [Troubleshooting](docs/agents/tools/memory/troubleshooting.md): What to do when your agent's memory isn't served, doesn't find a file or searches by words only

<!-- docs-tree:end -->

## Code

- [`main.rs`](src/main.rs): the program
- [`mcp.rs`](src/mcp.rs): its MCP tools
- [`semantic.rs`](src/semantic.rs): search by meaning
