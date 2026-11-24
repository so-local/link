# Move Files

Find what a move or a delete leaves behind, holds on its way, and copies aside. [Agent Files](files.md) lists what an agent keeps while it's here.

```sh
lnk agent list --here
```

## `~/.lnk/safe/agents/transfers/moved/<id>.toml` (600)

A note left when an agent moved away: where to (`[to] name`, and
`cloud = true` for a cloud account that runs it, whose Home [`lnk storage push`](../../../../../../storage/storage/docs/services/storage/guide.md#back-up)
and `pull` then reach, as `lnk agent list --json --here` says), and
when.
A note a newer release wrote is refused, naming [`lnk upgrade`](../../../../../../../docs/more/modules/guide.md#upgrade), never
taken for none, so a rollback never forgets where an agent went. The
move journal beside it, `~/.lnk/safe/agents/transfers/moving/<id>.toml`,
is refused the same way, so a move under way is never forgotten.

## `~/.lnk/data/agents/move/<name>-<date>` (700)

A moved agent's own folder and its Home, when it's its own, as `agent/`
and `home/`, kept 30 days, or as `~/.lnk/safe/agents/policy.toml`'s `keep_moved` says, so
it comes back whole if it returns. It's kept apart from `~/Link/Archive`,
which is the archive's. A deleted agent whose files can't be one archive
is kept here too.

## `~/Link/Archive/<name>-<date>.tar.gz` (600)

A deleted agent's state, its settings, sandbox file and files, as one
[archive](../../../../../../tooling/archive/docs/tooling/archive/README.md)
whose `archive.toml` records its deletion, kept as long as
`~/.lnk/data/agents/move/`. An archive you exported or put there yourself
never expires.

## `~/.lnk/data/agents/harness-history/<name>-<date>` (700)

The harness homes of an agent whose move wasn't seen running where it
went, without the keys a harness keeps in them, kept as long as
`~/.lnk/data/agents/move/`. Such an agent keeps its whole folder on the
box too, but each `.env` at a home's top or in a folder at its top
(`.<harness>/.env`) is deleted first, and the move says so. Its harnesses wrote them, so they're
data, never in a Safe.

## `~/.lnk/safe/agents/transfers/receiving/<id>/` (700)

An arriving agent's copy and switch, by its ID, with `move.json` (its
move, and when its copy came), before it's put in place; removed then,
when its switch fails, or 7 days after a copy no switch came for.

## `~/.lnk/safe/agents/transfers/sending/` (700)

Where a move copies a file over 8 MB, or memory's index, aside, unnamed,
to hash and send it as it was hashed.

## `~/.lnk/safe/agents/<name>/paused`

The agent is paused for a move's switch: what carries its connections
takes no message; `paused.quiet` beside it, once it stopped. A start
removes both.

## `~/Link/Agents/.restoring-<random>`

Where a restore unpacks a backup, made new for each restore, and removed
a week after a restore cut short left it.

## `~/.lnk/safe/agents/places/.spool/` (700)

Where a hosted Home's push or pull, by [`lnk storage push`](../../../../../../storage/storage/docs/services/storage/guide.md#back-up) and `pull`, copies
a large file aside to hash it, and unpacks a pull before it's put in
place.
