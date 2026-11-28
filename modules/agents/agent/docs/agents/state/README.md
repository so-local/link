# State

> State Is What Makes Your Agent Yours

**An agent's state is the files and folders the agent has access to, and the
ones that make its tools work.** Its instructions, skills, memory,
conversations, and the folders you give it are all state, and state goes
with the agent wherever it moves.

## Files the Agent Changes

An agent reads and writes its own files as it works. [Agent Files](files.md)
lists each one by purpose, and [Stores](../../../../stores/docs/agents/more/stores/README.md)
explains the folder, SQLite file, or encrypted bucket each collection is kept in.

## Safe the Agent Can't Touch

The [Safe](../../../../../storage/safe/docs/config/safe/README.md) keeps the
agent's settings, sandbox rules, secrets, and credentials. The agent works
under the Safe but never reads or changes it, and
[Agent Configuration](../more/settings/README.md) explains each file.

## State on the Move

[Move Files](moves.md) says what a move or a delete leaves behind and copies
aside. [Export Agent State](export/README.md) captures an agent's portable
state for inspection without starting a model request.

<!-- docs-tree:start -->

## Docs

- [Agent Files](files.md): Find your agent's saved files and Link's private runtime state, grouped by purpose
- [Move Files](moves.md): Find what a move or a delete leaves behind, holds on its way, and copies aside
- [Export Agent State](export/README.md): Capture your agent's portable state for inspection without starting a model request

<!-- docs-tree:end -->
