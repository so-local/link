# Stores

> Stores for Your Agent’s Collections

**Everything your agent keeps is kept at a place: the agent's folder on
this machine, a SQLite file, or an encrypted bucket of the agent's own.**
A store driver keeps each place outside the agent's sandbox, and the agent
and the agent's tools reach every place through the same storage bridge.

## Places Where Collections Live

An agent's things are [six collections](../../../../../agent/contract/spec/docs/config/specs/guides/agent.md#storage): its Home, instructions, skills,
memory, conversations and schedule.
Local Link keeps each collection at one of three places, by that place's
driver, outside the agent's sandbox. [File Store](../../../../file-store/docs/agents/more/stores/file.md)
keeps ordinary files, [SQLite Store](../../../../sqlite-store/docs/agents/more/stores/sqlite.md)
keeps rows in a local database, and [Bucket Store](../../../../bucket-store/docs/agents/more/stores/bucket.md)
keeps encrypted remote objects.

### `file`

`lnk-file-store` keeps every collection as ordinary files in the
agent's folder on this machine, `LNK_STORE_DIR`, which you read and
edit as they are. A conversation's events are appended to a log, with a
readable copy beside it, and [the driver](../../../../file-store/docs/agents/more/stores/file.md) follows no link within the
agent's folder.

### `sqlite`

[`lnk-sqlite-store`](../../../../sqlite-store/docs/agents/more/stores/sqlite.md) keeps all but `home` as rows of one SQLite file in the
agent's folder, which moves with the agent's files.

### `bucket`

[`lnk-bucket-store`](../../../../bucket-store/docs/agents/more/stores/bucket.md) keeps every collection as objects in the agent's
bucket, the first encrypted one [`lnk storage`](../../../../../../storage/storage/docs/services/storage/README.md#storage) connected for it, safe on
one machine at a time.

## Choose Where Collections Are Kept

An agent's specification says in its `[storage]` which place the agent prefers for
each collection, and the agent's machine decides. The machine's
`~/.lnk/safe/agents/policy.toml` lists the places the machine supports,
and [the agent's request](../../../../../../../docs/config/README.md#where-an-agents-things-are-kept) holds where the machine lists it.
A place whose driver isn't installed is installed as a module.

### Show Where Your Agent Keeps Each Collection

The agent's specification shows the place your agent currently uses for each collection.

```sh
lnk agent spec show
```

### Keep Conversations Through a Driver

Read how conversations stay in local logs, compressed history, or a store
driver in [Link Harness](../../../../../harnesses/link/docs/agents/link/conversations.md#keep-them-smaller-or-in-a-database).

## Write a Store Driver

A driver is a program, `lnk-<name>`, that answers `contract`, `call`
and `serve`, which the storage bridge invokes outside the agent's
sandbox. The [store interface](../../../../contract/docs/agents/more/stores/protocol.md)
defines a driver's requests and the reusable checks a driver passes. Tools
reach files held outside their sandbox through the
[files bridge](../../../../contract/docs/agents/more/stores/files-bridge.md).
