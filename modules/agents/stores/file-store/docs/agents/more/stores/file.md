# File Store

`lnk-file-store` is the `file` place's driver: an agent's collections
in its folder on this machine (`LNK_STORE_DIR`), as a person reads and
edits them (`link_store_contract::store::file`). It keeps every
collection, [its contract](../../../../../contract/docs/agents/more/stores/protocol.md) saying `serve`, `jobs` and `files`, and the
storage bridge runs it outside every sandbox.

## Files by path

A file by its path stays a plain file there
(`instructions/instruction.md`). Its version is an opaque revision
reserved from durable metadata in `.lnk/revisions/` beside the collections, independently
of the file’s contents. A driver write uses the expected revision and
renames the new bytes into place under the folder’s lock.

### Revision Implementation

The shared [file revision implementation](../../../../../contract/docs/agents/more/stores/protocol.md#file-backed-revisions)
reserves versions durably, detects ordinary outside edits, and defines
which metadata must accompany backups. This driver delegates those
operations to the same implementation it uses for path confinement.

## Conversations

A conversation is its folder, `chats/<thread>/`: its events a
log only added to (`events.jsonl`, each line its `seq` first), its copy
for people (`conversation.md`), its tools (`tools.json`) and its turn's
lease (`turn.json`). A conversation kept `compact` goes on compact.

### Narrowed before its first event

A conversation narrowed before its first event is its folder of
`tools.json` and a `conversation.md` of its first line, which names its
thread.

## No link followed

It runs outside every sandbox, so it opens each part of a path from the
folder before it, never through a link
(`link_store_contract::store::under`): a link in the agent's folder is
refused, never followed, and a thread whose folder would be more than
one name is refused.

## Jobs

It keeps jobs too, in `schedule/`, as the agent's scheduler finds them,
each the job as JSON, moved between its places by a rename. A job's ID
there is one name of a folder, not starting with `.`.

### Its files

A job waiting is `jobs/<id>.json`, one being fired `firing/<id>.json`,
one whose firing ends `firing/<id>.ending`, and one ended
`ended/<id>.json`; `day.json` is the day's
count, and `mark` the mark.

## Its tests

`tests/contract.rs` runs the file driver directly and behind the store bridge through the [shared driver checks](../../../../../contract/docs/agents/more/stores/protocol.md#test-a-driver), including `check_jobs`, `check_files`, and `check_narrowing`.
