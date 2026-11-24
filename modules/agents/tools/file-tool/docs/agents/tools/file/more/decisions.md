# Decisions

Why `file`'s calls are Claude Code's, why it keeps to `[files]` before
its sandbox does, what it narrows per call, and how it works through a
files bridge and in a bucket.

## Why are its calls Claude Code's?

A tool is only as good as the model calling it, and current models are
trained on Claude Code's file tools. So `file`'s calls take their names
and arguments: `read{file_path, offset, limit}` as Read,
`write{file_path, content}` as Write,
`edit{file_path, old_string, new_string, replace_all}` as Edit, and
`glob{pattern, path}` and `grep{pattern, path, glob}` as Glob and Grep.

### Glob and Grep

Finding files and finding lines are two calls, as they are there, each
keeping the same limits, skipping and time. The sandbox, the grants and
the receipts fit around that shape.

### Their answers

Each answers in Claude Code's shape too: `read` numbers its lines, `glob` and `grep` give plain lists, and
`grep`'s `output_mode` gives the files that hold a match unless asked
for lines or counts, which spends fewer tokens on a first search. Grep's
`-i` isn't taken, since an argument's declared name starts with a
letter, so `(?i)` in the pattern matches any case. Its context lines,
`type`, `head_limit` and `multiline` are left out.

### Numbered lines and the stale check

A line's number is shown, never kept: the stale check compares the
SHA-256 of the whole file, and the read's receipt names the numbered
text it gave, with `numbered` among its transformations, so the receipt
still checks against what the model was given.

## Why does `file` keep to `[files]` before its sandbox does?

The sandbox decides: `file`'s reads Home and the agent's instructions,
skills and conversations, writes only `[files] write`, reaches no
network and runs only `lnk-file-tool`. The tool checks the same folders
first so a refused write says in words where the agent may write, rather
than a system's error. It never follows a link, writes a file whole
beside it and moves it in place, and reads at most 24 KB a call, whole
lines, so a harness keeps the whole answer.

## Why does each conversation read only its own chat?

A conversation's transcript holds what its people said. A webhook, a
member of a shared chat, or a teammate given the agent's ear must not
read the owner's other conversations through `file`. The sandbox serves
a whole session, so it reads every conversation. The tool's server
therefore narrows `chats/` per call, from the conversation and standing
the harness names, and refuses any call that names neither. Memory
remains the owner's way to search across conversations.

## Why does a write refuse a file changed since its read?

A person editing a file while the agent works, or two conversations
writing one file, would otherwise lose a change silently. The server
keeps the SHA-256 of what each file held when the session last read or
wrote it, and the next `write` or `edit` names it: an edit whose text
is still there would otherwise land on a file whose other lines changed
unseen. The sandbox refuses the call
when the file differs, as the files bridge refuses a stale version. The
version never reaches the model. A file never read is written, so a new
file needs no read first.

## Why do `glob` and `grep` keep to `.gitignore`?

What a `.gitignore` names is usually built or fetched, like
`node_modules/` or `target/`. Walking it would spend a search's budget
of files on what the agent never edits and bury what it would. Each
keeps to the same rules as git, deeper files over shallower ones, and
counts what it passed over, so the model knows to name a folder when it
wants one that is ignored.

## Why can `file` work through a files bridge?

So an agent whose files folder isn't on its machine (a bucket's folder,
on a machine that keeps nothing of it) still reads and writes it, with
the same tools and limits. The bridge (`link_store_contract::files`)
runs outside the sandbox, holds the folder's credential and key, and
takes a call only from the agent bound, proved with its key
(`link_sandbox_bridge`); a write of a file read is made only from the version
read, so two writers never lose one silently. The other way, the folder
mounted into the sandbox, would put its credential where the agent is.

## Why rclone's crypt format for a folder in a bucket?

Because it's published and tested, [`lnk storage`](../../../../../../../../storage/storage/docs/services/storage/README.md#storage) already writes it, and a
folder stays readable by standard tools given its key. Each object is
encrypted on its own, its name too, so single objects are read and
written with the bucket's version conditions. A format of Link's own
would buy nothing and be Link's alone to get right.
