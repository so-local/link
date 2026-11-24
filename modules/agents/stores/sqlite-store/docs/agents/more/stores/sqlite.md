# SQLite Store

`lnk-sqlite-store` is the `sqlite` place's driver: an agent's things as rows
of one file on this machine, `events.sqlite` in the folder its caller
names (`LNK_STORE_DIR`, the agent's `chats/`), so it moves with
the agent's files. It keeps [every collection but `home`](../../../../../contract/docs/agents/more/stores/protocol.md), and serves.

## Its tables

Each kind of thing it keeps is a table of the same file, which it
makes at the table's first write.

### Conversations

A conversation's events and its lease, and in `tools` the tools each
conversation was narrowed to.

### `files`

Its files by path: instructions, skills and memory's, each a row with
its version, from 1. The next tick of its agent's clock is in
`file_clocks` at each write, so a file removed and made again never has
a version it had before.

### `jobs` and `job_days`

The agent's scheduled jobs, each in `jobs` by its place and ID, and the
day's count and the waiting jobs' mark in `job_days`, a row an agent.

## One writer at a time

SQLite serializes writers across processes. Compound changes use
`BEGIN IMMEDIATE` before reading and changing related rows, so the
next event, a lease takeover, a job move, and a file revision are chosen
while that write lock is held. Single-statement updates and removals
remain atomic database operations.

### A reader

A reader only reads: it opens the file read-only, and a file that isn't
there, or doesn't hold the tables yet, holds nothing.

## Its tests

Its `tests/contract.rs` runs it through `link_store_contract::store::check`,
`check_jobs`, `check_files` and `check_narrowing`, run here and behind
the store bridge.
