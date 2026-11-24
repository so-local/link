# Memory

> Memory Knows Where It Put Things

**Your agent's memory is a search over the agent's own files.** The agent
finds the documents in its `memory/` folder and its past conversations by
their words, or by their meaning with a model that runs on your computer,
and keeps notes that it reads at every message.

<!-- quickstart -->

## Memory Start

Start Link Memory for your agent’s files and conversations. Learn which [folders Memory searches, which notes Memory saves, and which harnesses Memory supports](guide.md).

```sh
lnk agent start
lnk agent memory start
```

## Check Which Memory Engine Runs

Check which memory engine is selected and where your agent reaches it. Learn about the [engine choices and stored data](guide.md).

```sh
lnk agent memory
```

## Install an Embedding Model

Install an embedding model so that a question finds a note that shares none of the question's words. You can then [search by meaning](guide.md#search-by-meaning).

```sh
lnk model install qwen3-embedding
```

## Inspect Memory Calls

Inspect memory tool calls in the sandbox log. You can also see [how each harness connects the tools to the running agent](guide.md#in-each-harness).

```sh
lnk sandbox log show
```

## Integrate an Engine

Implement or inspect an engine that Local Link serves to a harness with the
[memory engine interface](../../../../contract/docs/agents/tools/memory/more/protocol.md).
The interface defines adapter startup, shared sessions, binding credentials,
and embedding authentication.

## Read PDFs and Word Documents

Add PDF and Word text extraction with [Document Converter](../../../../document-converter/docs/agents/tools/memory/documents.md). The converter's [contract](../../../../document-converter/contract/docs/agents/tools/memory/documents/more/contract.md) explains the child process’s formats, limits, and typed answer.

## Learn How Memory Works

Read why Memory is built as it is in [Decisions](more/decisions.md), how the tool's process and calls fit together in [Developing](more/developing.md), how each agent's index is written and kept whole in [The Index](more/index.md), how it learns what changed in [How Memory Stays Current](more/freshness.md), what it embeds first in [What Is Embedded, and When](more/embedding.md), and what to do when the tool fails in [Troubleshooting](troubleshooting.md).

<!-- docs-tree:start -->

## Docs

### Pages

- [Memory Guide](guide.md): Start Link Memory, add searchable documents, and choose whether searches match words or meaning
- [Memory's Index](more/index.md): How Link Memory keeps each agent's index, writes it in the background, and keeps it whole through a crash, a schema change or a full disk
- [How Memory Stays Current](more/freshness.md): How Link Memory learns that a file changed without walking the agent's folder to answer a call
- [What Is Embedded, and When](more/embedding.md): How Link Memory chooses which passages to embed next, how one thread embeds for every agent a service serves, and how the vectors are kept

### More

- [Developing](more/developing.md): `lnk-memory-tool` is Link Memory, the memory engine a Link agent uses by default: an MCP server over stdio that searches, reads and lists the agent's files and past conversations, keyless and local
- [Decisions](more/decisions.md): Why Link Memory is named, writes, indexes, is sandboxed and searches as it does
- [Troubleshooting](troubleshooting.md): What to do when your agent's memory isn't served, doesn't find a file or searches by words only

<!-- docs-tree:end -->
