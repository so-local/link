# Developing

`lnk-file-tool` is Link File: an MCP server over stdio that reads and
writes an agent's files, its calls in a sandbox. How to use it: [Link File](../README.md).

## Build and test

From the repository root, run `cargo build -p link-file-tool`, then `cargo test -p link-file-tool`.

## How it's put together

`serve [--no-find] [--no-diffs] -- <command>...` is the tool, its flags
leaving `glob` and `grep`, or each answer's diff, out of what it lists, says and
answers (`Offer`, `src/offer.rs`). `--write`, `--whole`, `--team-write`
and `--team-read` repeat `apply`'s grants, from which `Offer` generates
the instructions it gives at `initialize`. Its calls are declared in
`actions.toml` with the names and arguments of Claude Code's Read,
Write, Edit, Glob and Grep, each argument with dashes (`file-path`) and
called and handled with underscores (`file_path`), as [Argument Names a
Model Calls](../../../../../../../../../packages/actions/docs/more/developing/modules/catalog/tools.md#argument-names-a-model-calls)
explains.

### Each call's conversation

Before a `read`, `glob` or `grep` runs, the server checks its
`file_path` or `path` against the conversation the call's `_meta` names
(`link.local/thread` and `link.local/standing`, as `Caller` in
`src/places.rs` reads them). Of `chats/`, only `chats/<folder>/` of the
call's own conversation passes, where `<folder>` is the thread's name
with each `:` a `-`. A stranger's call, or one naming no conversation
or standing, reaches none. The check runs in `answer_with`, so it holds
on a machine and through the files bridge alike.

### Its sandboxed process

It touches no file: it starts `<command>` at the first call, which Link
makes [`lnk sandbox run`](../../../../../../../../security/sandbox/docs/security/sandbox/guide.md#sandbox-guide) with the sandbox file's flags, then
`lnk-file-tool apply --files <folder> --agent-folder <folder> --write
<folder>...`, and keeps it for the session (`Sandboxed`,
`src/sandboxed.rs`). `--whole` stands for the whole files folder, `.`,
which [`lnk sandbox tool add`](../../../../../../../../security/sandbox/docs/security/sandbox/reach.md#call-a-tool-that-runs-outside) would refuse as a path from here.

### Each call

Each call is a line on its stdin, `{"name", "arguments", "expect"}`,
and its answer a line back, `{"ok": ...}` or `{"error": "..."}`. One
that ends is started again at the next call; one that never starts says
what it wrote on stderr.

### Its limits

Each size, count and time it holds to is in `src/limits.toml`, read
once into `Limits` (`src/lib.rs`): the largest file it reads or writes
(`file_max`, 4 MiB), the most one `read` gives (`read_max`, 24 KB), the
longest diff it answers with (`diff_max`, 16 KB), and what `glob` and `grep` give,
looks at and compiles. Its test, `its_limits_read`, checks they hold
together, such as a read and a diff each within what a harness keeps of
an answer.

### What a model sees

Each answer's `content` is its text, fenced where it holds what a file
says. Its `structuredContent` is the same answer without that text:
what a `read` read, a line `grep` found and a change's `diff` are in
the fenced text alone, so a client that gives its model the structured
content never shows it bare. A `read` keeps its
[read receipt](#text-read-receipts) there, which names the text by its
digest, and a reader checks it against the `content` text unfenced
(`link_untrusted::unfenced`).

### A message's most

A message is at most `message_max()` (`src/lib.rs`), which is twice a file's most
escaped as JSON escapes it at worst (six bytes a byte), so a write under
4 MiB always fits. A call past it is refused before it's sent. An answer
past it stops the sandboxed process, its output closed, so the call
never waits on a full pipe.

### A file's version

Each `read`, `write` and `edit` answers with the file's `version`,
the SHA-256 of what it read or wrote. The server keeps it by path for
the session (`Versions` in `src/lib.rs`) and takes it out of the answer.
A later `write` of that path sends it as `expect`, and `apply` refuses
the write when the file's bytes no longer match. An `edit` checks the
file is still what it read just before it moves the new text in.

## Applying a call

The sandboxed `apply` does each call's work on the files folder.

### `apply`

It checks the folders before the sandbox does, so a refusal says why in
words: a path is relative, without `..`, and never through a link. It
routes a path by its parsed parts, so `./instructions/x.md` and
`.//instructions/x.md` are both the agent's own folder.

### What it writes

A write is held by `home_writable` in `src/places.rs`, the one check
`bridge` makes too. The path must be in a folder `--write` names, never the folder
itself. With `--write .` it may be anywhere but `chats/` and the
agent's own `instructions/` and `skills/`, which are written only where
named. A dot file a person's tools run is never written.

### Text Read Receipts

A text read's structured result includes `read`, a
[`ReadReceipt`](../../../../../../../../../packages/files/docs/more/libraries/files/files.md#text-read-receipts).
It identifies the complete source snapshot, the selected byte span, and
the exact returned text after its counted pagination prefix, the text
the `content` gives inside its fence. `cut` says
the source was not returned whole, including a single clipped line whose
line number is also the file's last. Local and bridged reads use the same
receipt. A later harness answer limit can still shorten delivery.

### A write

A write goes to a file beside the target, is synced to disk, then is
moved in place. It keeps the target's permission bits, but never its
setuid, setgid or sticky bit.
`write` and `edit` answer with the change from
`crate::diff::unified` as `diff`, from the text the file held, or
`/dev/null` for a file made; past 16 KB, or where the file held wasn't
text, `diff_cut` and the new text's `sha256` instead.

### `glob` and `grep`

Each builds one `Query` from its arguments (`src/find.rs`), for a walk
on disk and through the bridge alike. A glob, `glob`'s `pattern` or
`grep`'s `glob`, is made a regular expression, any case, matched
against a file's name, or its path below the folder searched when it
holds `/`. `grep`'s `pattern` is a regular expression of a line. Each
is compiled to at most 1 MiB, so a hostile pattern is refused.

### Its walk on disk

The walk reads each folder's `.gitignore` on its handle and
keeps a stack of their rules (`Ignore` in `src/find.rs`). The deepest
rule naming a path decides, and within one file the last. It counts what
it passes over in `Skipped` and stops at `files_max` files and folders,
`found_max` found, or `find_time_max_ms`, saying which in `stopped`.
Its time is given to the walk (`find`) by its caller, so a test gives it none and
sees it stop.

### `tests/calls.rs`

It lays the agent out as Link does, Home and its own folder side by
side. It runs the tool as `apply` alone, a session's calls in one, and on
Linux under `lnk-sandbox run` from the core, told it may write a folder
the sandbox doesn't, to show the sandbox holds.

## Through the files bridge

`bridge --socket <path> --store <path> --as <agent id> --account <account>` is the tool where
Home is kept in a bucket: each call goes to the files bridge
(`link_store_contract::files`), proved with the agent's key from the
keyring (`--keyring`, else this machine's). `tests/bridged.rs` runs it
against a stand-in bridge over an encrypted folder.

### Instructions and skills through the storage bridge

A path in `instructions/` or `skills/` goes to the storage bridge at
`--store` instead (`link_store_contract::store`, its `file_read`,
`file_write` and `file_list`), proved with the same key, so the store
answers for that agent alone. `Bridges` holds both clients, and
`Kept` says which a path is in: Home, a team's folder or the agent's
own. Writes keep `[files]`'s grants through the same `home_writable`.

### The store's versions

The store's versions are numbers, carried as text in the `Seen` as the
bucket's are; a stale one answers as the bucket's `Conflict` does.
`tests/bridged.rs` serves a stand-in storage bridge over the file
driver's folder (`store::file::Folder`).

### The version a write names

A `write` names the version of the file it last read or wrote
(`Expect::Version`, kept by path in a `Seen` for the session), so a
file changed since is refused; of a file it never saw, the version it
reads first, or `Absent` for a new file. An `edit` names the version
last seen in the same way, and of a file it never saw, the version it
reads. On a machine, the server's `Versions` gives a `write` and an
`edit` alike the version last seen, which the sandbox checks under the
folder's lock.

### Paging a read

A `read` from line 1 keeps the file's text in the `Seen` (one file, the
last so read); a `read` from a later line (`offset`) pages that text, so a 4 MiB
file's pages read it once, not once a page, and a `write` or `edit`
of it forgets it.

### `glob`, `grep` and teams

`glob` and `grep` list the folder through the bridge, and `grep` and reads each file of at
most 1 MiB, stopping at `find_time_max_ms` as it does on disk. A path under `teams/<team>/`, or `teams/<account>/<team>/`
(an account's ID is never a team's name), goes to that team's folder
(`TeamRef`, with `read_in`, `write_in` and `list_in`): the bridge says
at each call what of it the agent reaches.

### A team's folder kept elsewhere

`serve --team-bridge <team>`, with the bridge's `--socket`, `--as`,
`--account` and `--keyring`, serves the files folder as `serve` does but
sends each call at `teams/<team>/`, its `file_path` or a `glob`'s or `grep`'s `path`,
through the files bridge (`serve_teamed`, `BridgedTeams`). The agent's
bridge serves that files bridge for each of its teams whose folder an
account hosting agents keeps. A `glob` or `grep` with no `path` looks in the files
folder alone. `tests/hosted.rs` runs it against a stand-in folder
service, through the agent's bridge's `RemoteTeams`.
