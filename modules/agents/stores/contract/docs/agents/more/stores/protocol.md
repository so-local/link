# Interface

The commands a store driver, `lnk-<name>`, answers for an agent's
collections. The [Files Bridge](files-bridge.md) separately describes
bucket-backed Home and team-folder access.

## Store drivers (`lnk-<name>`)

The contract is
[`store.rs`](https://github.com/so-local/link/blob/main/modules/agents/stores/contract/src/store.rs).
A driver keeps an agent's things at a place: Link's are
[`lnk-file-store`](../../../../../file-store/docs/agents/more/stores/file.md),
[`lnk-sqlite-store`](../../../../../sqlite-store/docs/agents/more/stores/sqlite.md),
[`lnk-bucket-store`](../../../../../bucket-store/docs/agents/more/stores/bucket.md).

### Declared Once

The protocol's commands, `contract`, `call`, and `serve`, are declared
once, in the contract crate's
[`actions.toml`](../../../../actions.toml), with what each answers in
its [`types.toml`](../../../../types.toml): `contract` answers
`contract`, `call` one `reply`, and `serve` `served`, one `reply` a
line. A driver writes each as its declared type, so a reply its
declaration refuses is an error, never written. Each driver lists the ones
it answers under `[implements]` in its own `actions.toml`, and its
command line is generated from that, so every driver reads its command
the same way.

### Where Link Harness uses one

Link Harness keeps an agent's conversations with a driver when its specification
says the driver's name (`[storage] conversations = "sqlite"`). A
conversation a driver keeps has an empty `store.<name>` in its folder,
which says so to Link Harness, memory and [`lnk agent chat show`](../../../../../../agent/docs/agents/chat/README.md#talk-to-it-here).

## The store bridge

Link Harness calls a driver through the agent's store bridge: a thread
of the agent's bridge (`lnk agent bridge`), outside the sandbox,
listening at `link-store.sock` in the harness's own folder (600),
which runs the driver there.

### Kept running or run once

A driver whose contract says `serve` is kept running, at most 4 of it
at once, each answering call after call on what it holds (a connection
to its store). At most 2 are kept idle, each stopped once
it's been idle for 60 seconds, whether or not another call comes, and
the rest with the bridge. Any other driver is run once a call. A call
is stopped past 30 seconds.

### Kept by a Harness Without a Bridge

A harness started without a bridge runs the driver in its own process
the same way: at most 2 kept idle for each folder and environment it
runs the driver with, so an agent's conversations cost one process
rather than one for each of a turn's dozen calls. Each idle for 60
seconds is stopped, whether or not another call comes. One that stops
is started again at the next call, and those left end when the harness
does, their requests' pipe closed.

### Each call

Each call is a bridge's call whose action is the `Request` below,
proved as the agent the request names, answered with one `Reply` line,
a driver's `{"reply": "error"}` answered as the error envelope. A call
naming another agent's conversations, or proved as any agent but the
one whose bridge it is, is refused
(`link_store_contract::store::serve_bridge`).

### Its limits

It holds at most 16 proved calls at once, 8 of one agent's (one more is
answered `busy`), waits at most 5 seconds for a proved call's request,
and runs at most 4 drivers at once.

### Links and its socket

It refuses a call while its driver's folder or the agent's own folder
is a link. It never changes its socket's mode, which is in a folder the
harness writes: the proof decides who's answered.

### Which adapters get it

Legacy adapters require the agent's folder layout, writing
`chats/` but not the folder around it.

### Outside the sandbox

The driver's program, and whatever it needs to reach its store, stay
outside the sandbox.

### Without a bridge

Without a bridge (memory, the step of `lnk agent chat show`, a
conversation kept by another driver than the specification's now), a driver runs
where its caller runs, in its caller's sandbox, which reads its
program.

### `LNK_STORE_DIR`

The folder a driver on this machine keeps what it keeps in, the agent's
`chats/` in its own folder (`agent/chats`), so it moves
with the agent. Whoever runs it may set more in its environment, such
as where its own settings are, as its docs name them
(`link_store_contract::store::Driver::with_env`).

### `LNK_STORE_AGENT`

`LNK_STORE_AGENT` names the agent identity selected by the storage bridge for the driver’s request. It is a protocol input supplied by Link, rather than a user setting.

## `lnk-<name> contract`

Prints `link_store_contract::store::Contract` as JSON: `{"version",
"serve"?, "jobs"?, "files"?}`. Each left out, it doesn't. The store
bridge asks it once a program.

### `version` and `serve`

`version` is the version of this contract it speaks, and `serve` is
true when it answers `lnk-<name> serve`. The contract's `protocol.toml`
declares the version this release speaks, 1, and the oldest a caller
still reads. A caller states no version back, so it reads a driver's
within that window and refuses an older or a newer one by name, naming
[`lnk upgrade`](../../../../../../../../docs/more/modules/guide.md#upgrade).

### `jobs` and `files`

`jobs` is true when it keeps agents' jobs, the `job_*` ops, and `files`
when it keeps agents' files by path, the `file_*` ops. The bridge never
sends a `job_*` op to a driver that doesn't say `jobs`, nor a `file_*`
op to one that doesn't say `files`: the call is refused, saying so.

## `lnk-<name> call`

Reads one `link_store_contract::store::Request` as JSON on stdin,
`{"op", "agent": {"id", "owner"}, ...}`, and prints one `Reply`,
`{"reply", ...}`, exiting 0; `{"reply": "error", "error"}` when it
fails. The agent is named as Link names it (`LNK_AGENT_ID`,
`LNK_OWNER`; for an agent without them, its name and no owner). Each
op of a conversation follows, its arguments in brackets, and its reply.

### `append`

(`thread`, `event`): writes the event, a JSON object whose `seq` is its
number from 1, only if its number is free and the next: `done`, else
`taken` with the conversation's last number (`last`). Of two writers of
one number, on any machines, one writes it. An event written is never
changed, and nothing of it but `seq` is the driver's to read.

### `read`

(`thread`, `first`): the events numbered `first` and on, in order:
`events` (`events`), none for a conversation it doesn't keep.

### `last` and `threads`

`last` (`thread`) answers the last event's number, 0 for none: `last`.
`threads` answers each conversation it keeps of the agent, with its
last number: `threads` (`[{"thread", "last"}]`).

### `remove`

(`thread`): forgets a conversation, its events and its lease: `done`.
The tools a list narrowed it to stay (`tools_read` answers them still),
so a conversation removed is never made again with more.

### `take`

(`thread`, `holder`, `lapse_ms`): takes the conversation's lease, which
says which holder runs its turn, for
`holder`: `took` with `took`, which is `took` (no turn ran), `lapsed`
or `held` (another's, renewed lately). `lapsed` is taken from a holder
not renewed for `lapse_ms`, judged to the millisecond, never by a whole
second: its turn crashed, and there's one taker only.

### `renew` and `end`

`renew` (`thread`, `holder`) answers `done`, or `lost` when the lease
isn't `holder`'s any more. `end` (`thread`, `holder`) lets `holder`'s
lease go, nothing when it isn't its: `done`.

### `leases`

(`lapse_ms`): every lease of the agent's conversations, `leases`
(`[{"thread", "holder", "started", "renewed", "lapsed"}]`), in seconds
since 1970 by the driver's clock, `lapsed` judged by `lapse_ms`.

## Jobs

A driver whose contract says `jobs` keeps the agent's scheduled jobs
too, for a scheduler whose agent runs only while it answers, such as [Link Scheduler](../../../../../../tools/scheduler-tool/docs/agents/tools/scheduler/more/developing.md) with `--store-bridge`: they outlive it on the machine, and any machine it's
bound to next reads them.

### A job

A job is `{"id", "spec", "due"}`: its ID (1 to 256 bytes, no control
character), its specification (JSON, at most 1 MiB, none of it the driver's to
read) and when it's due (seconds since 1970). Each is in a `place`,
`waiting`, `firing`, `ending` or `ended` (kept with its outcome, never
due), and one ID may be in more than one.

### `job_put`, `job_get` and `job_remove`

`job_put` (`place`, `job`) keeps the job in `place`, in place of any
there with its ID: `done`. `job_get` (`place`, `id`) answers `job`
(`job`, the job, left out for none). `job_remove` (`place`, `id`) answers
`found` (`found`), whether it was there.

### `job_list` and `job_count`

`job_list` (`place`) answers every job in `place`, in no order: `jobs`
(`jobs`). `job_count` (`place`) answers how many, none read: `count`
(`count`).

### `job_shift`

(`id`, `from`, `to`): moves the job from `from` to `to`, in place of
any there with its ID, whole: `found` (`found`), whether it was in
`from`. Of two shifts of one job from one place, on any machines, one
is made, as of two appends of one number one writes it.

### `job_next`

When the soonest job waiting is due, never one being fired: `next`
(`next`, left out with none waiting). A store off the machine answers
what's due for every agent from the same.

### `job_day` and `job_set_day`

(`day`, `sent`): the day's count of the agent's messages, the day as
days since 1970 in the agent's time zone: `day` (`day`, `sent`; `0` and
`0` for none kept), and `done`.

### `job_mark`

A number that differs once a job is put in, shifted to or from, or
removed from `waiting`, which a runner watches between its looks:
`mark` (`mark`).

## Files

A driver whose contract says `files` keeps the agent's files by path. A
driver that doesn't keep a collection's files refuses any `file_*` op
of it, `{"reply": "error"}`.

### A file's path

A path of its instructions, skills, or memory, its first part naming which
(`instructions/instruction.md`, `skills/greet/SKILL.md`, `memory/notes.md`): relative, no
part empty, `.` or `..`, at most 1,024 bytes
(`link_store_contract::store::check_file_path`).

### What the bridge checks

The store bridge checks every `file_*` op's path, and the size and
base64 of what it writes, before the driver runs, refusing any other.

### `file_read`

(`path`): `file` (`file`: `{"data", "version"}`, its bytes as base64,
at most 4 MiB for instructions and skills, or 16 MiB for memory, and its version;
left out for none).

### `file_list`

(`folder`): the paths of the agent's files under `folder` (`skills/`),
in order, at most 4,096 (`link_store_contract::store::FILES_MAX`):
`files` (`files`). A folder of more is refused, never answered cut
short; a caller refuses a longer answer.

### `file_write`

(`path`, `data`, `version`): writes the file at `path` with `data`
(base64, within the same decoded file-size limits) if its version is still `version` (`0`: there's
none): `wrote` (`version`, its new version), else `stale` (`version`,
its version now, `0` for none). Of two writers of one version, one
writes; the other is told `stale` and reads again.

### Versions never come back

A file is never given a version it had before, even made again after
its removal, so a writer still holding one from before is told `stale`.

### File-backed Revisions

The shared file implementation keeps revisions beside the collections.
A file-backed driver uses this policy when it reads, writes, or removes
a logical file, rather than deriving versions from file contents.

#### Revisions Persist

The [revision reader](../../../../src/store/file.rs)
keeps a durable counter and each file’s content and filesystem identity.
A driver write reserves and syncs its next revision before changing bytes.
Writing equal bytes again or recreating a removed file receives a new
revision. Reopening unchanged data retains its existing revision. A crash may
spend a reserved revision without changing the file, so callers compare
versions for equality rather than expecting consecutive numbers.

#### Edits Outside the Driver

When a later read observes changed contents or filesystem identity, it
records a new revision before returning the file. A file that changes
while being read is refused. These checks detect ordinary human edits;
callers still use revision equality rather than treating a version as a
content hash or timestamp.

#### Keep Revision Metadata

Keep the revision metadata together with the agent’s collections during
backup and transfer. Removing it discards the history that prevents an
old writer from matching recreated data. Exhaustion, a missing established
counter, an inconsistent revision record or an unsupported metadata schema
refuses the operation instead
of silently resetting the counter.

### Object-backed Records

The object-store adapter keeps a logical file’s numeric revision inside
its versioned stored record, separately from the backend’s opaque
conditional-write version.

#### Stored Shape

The saved JSON record has `schema: 1`, a nonzero
`revision`, and `data` containing base64 text or `null` for a tombstone. Deleting a logical file retains a tombstone and its revision.
Reads and listings hide that tombstone, while recreation advances it, so
equal file contents do not revive an earlier caller revision. Invalid
records, unsupported future schemas and revision exhaustion refuse the
operation before mutation. A future schema asks for a Link upgrade.

### `file_remove`

(`path`, `version`): removes the file if its version is still
`version`: `found` (`found`), whether it did.

### `file_append`

(`path`, `data`): adds `data` (base64) at the file's end, made if
there's none, in one step: of two appends at once both are kept, one
after the other, never one within the other. `wrote` (`version`). One
that would grow it past its collection’s file-size limit is refused,
the file left as it was.

## A conversation's tools

The tools a conversation was narrowed to: a subagent's, its [`Link-Tools`](../../../../../../../../docs/more/modules/contracts.md).
A driver that doesn't keep them refuses these ops, `{"reply":
"error"}`.

### `tools_read`

(`thread`): `tools` (`tools`, a list, left out when nothing narrowed
it and it has every tool).

### `tools_list`

Every conversation a list narrowed, with what it may use, those removed
(their narrowing kept) and those narrowed before their first event
included, which `threads` lists neither of: `narrowed` (`narrowed`,
`{"<thread>": [<tool>, ...]}`). A move carries each from here, so none
arrives with every tool.

### `tools_narrow`

(`thread`, `tools`): narrows the conversation to `tools`, at most 256
tools' names (`link_store_contract::store::check_tools`, which the
store bridge checks), of those it may use, in one step: a tool it may
not use stays out, so of two at once the second narrows what the first
left, and a list never widens it. `tools`, what it may use now.

### Kept off the machine

A caller takes an answer naming a tool it didn't send, or one that
leaves `tools` out, as an error. A harness whose conversations are kept off the machine keeps
their tools only here, and fails a turn whose tools it can't read
rather than offer every tool.

## `lnk-<name> serve`

A driver whose contract says `serve` only. Reads requests as `call`
does, one a line on stdin, and prints each one's `Reply` as a line, in
order, until stdin ends, then exits 0; an error is that request's
`{"reply": "error"}`, and the next is read.

### One call at a time

A call still runs as one: the bridge sends the next only once it has
this one's answer, and stops the driver when an answer takes past 30
seconds.

### Driver Entry Point

A driver's generated command line reads its command, refusing unknown
commands, flags, and operands before a request is read, and maps it to
the contract's `store::Operation` by its words (`Operation::of`).
`link_store_contract::store::run`
takes that operation, its contract (built on `Contract::serving`, with
`jobs` or `files` for a driver that keeps them), and its answer. A
driver writes nothing else to its stdout.

## Test a driver

Use the contract’s reusable checks to exercise a driver directly and behind the store bridge. These checks establish the driver contract for the selected implementation and fixture, while a remote storage provider needs its own integration evidence.

### `check`

`link_store_contract::store::check` runs a driver through all of it,
eight writers of one number at once and four takers of one lapsed lease
included.

### `check_jobs`

Runs a driver that keeps jobs through theirs, eight shifters of one job
at once included, and another agent's jobs its own.

### `check_files` and `check_narrowing`

`check_files` runs a driver that keeps files through theirs, eight
writers of one version and eight appends at once included, and another
agent's files its own. `check_narrowing` runs one through a
conversation's narrowed tools, eight lists at once included.

<!-- docs-tree:start -->

## Docs

- [Files Bridge](files-bridge.md): How an agent's Home kept in a bucket is reached from its sandbox, and the storage credential the bridge holds, both in `link-store-contract`

<!-- docs-tree:end -->
