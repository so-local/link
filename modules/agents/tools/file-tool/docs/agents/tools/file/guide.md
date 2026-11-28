# Files Guide

Enable File to read, write, edit, and find files within your agent’s granted folders. Set its sandbox’s writable paths before asking it to change a document.

```sh
lnk agent start
lnk agent tool add file
```

## Ask it to change a file

Ask it to "add tomatoes to my garden list" and it reads
`work/garden.md`, edits the line, and answers with what it changed.

### What it needs

You need the agents modules, which [`lnk up`](../../../../../../../../docs/more/modules/install.md#pick-your-modules) installs and which
bring Link File, and Link Harness. A new agent has `file` on, so asked
to change a file, its standing instructions or a skill, it does.
[`lnk agent tool add file`](../../../../../../agent/docs/agents/README.md#tools) turns it on again where a specification turned it
off, and each start then gives the agent its `file`.

## What your agent can do

Your agent sees five tools, each over a path in its files folder, such
as `work/garden.md`. They take the names and arguments of Claude Code's
Read, Write, Edit, Glob and Grep, the file tools current models are
trained on, so a model calls them as it already knows how. Link Harness
also accepts the corresponding slash commands with a JSON object after
the name, using the running agent’s enabled provider and grants.

### `file__read`

`/file:read` invokes `file__read`, for example
`/file:read {"file_path":"work/garden.md"}`. It gives a text file, at
most 24 KB a call, from line `offset` (the first, left out) and at most
`limit` lines, and says where it stopped so the next call reads on.

#### Numbered lines

Each line comes after its number and an arrow, as Claude Code's Read
gives it (`     1→tomatoes`), so a model quotes a line by its number, and
`file__edit` takes the text without them.

#### Images and PDFs

An image (PNG, JPEG, GIF or WebP) or a PDF
of at most 2 MiB it gives whole, as MCP's image or resource content, so
a model that reads them sees it. A larger one is refused, since a
model's provider may refuse it too.

### `file__write` and `file__edit`

`/file:write` invokes `file__write` to write a file whole (`file_path`
and its `content`), making it and its folders. `/file:edit` invokes
`file__edit`, which replaces `old_string`, given exactly as the file
holds it, with `new_string`, found once, or every time with
`replace_all`.

### `file__glob` and `file__grep`

`/file:glob` invokes `file__glob`, which finds files by a glob
`pattern` of their name, any case (`*.py`), or of their path below the
folder it looks in when the glob holds `/` (`tests/**/test_*.py`, where
`**/` is any folders or none). `/file:grep` invokes `file__grep`, which
finds a regular expression `pattern` (`fn \w+\(`), in the files its
`glob` names when given one, and `(?i)` at its start matches any case.
With no `path`, or an empty one, each looks in Home, never in the
agent's own folder.

### What `file__glob` and `file__grep` answer

Each answers as Claude Code's Glob and Grep do, a plain list: a path a
line from `file__glob`. `file__grep` gives what its `output_mode` asks
for: the files that hold a match, left out (`files_with_matches`), each
matching line as `path:line:text` (`content`), or each file's count of
them as `path:count` (`count`). A last line in brackets says what was
passed over and why it stopped early, where it did.

### What `file__glob` and `file__grep` pass over

Each passes over hidden files and what a folder's `.gitignore` names
(as git does, so `node_modules/` and `target/` are usually skipped).
`file__grep` also passes over files over 1 MiB and files that aren't
text. The answer counts each. Each looks at 10,000 files
and folders at most, for 10 seconds at most, and finds 100 at most.
When one stops early the answer says why.

### Its diffs

`file__write` and `file__edit` answer with the change as a unified
diff, as `git diff` shows one, so your agent sees what it changed. A
diff over 16 KB, or of a file that wasn't text, is left out: the answer
says so (`diff_cut`) and gives the new text's SHA-256 instead.

### Marked as the files'

What `file__read`, `file__glob` and `file__grep` give, and a change's diff, comes inside a fence, an opening `<untrusted ...>` line and a closing line with the same random ID, which tells your agent's model that it's data to read and never your instructions, since anyone may have written it. An image's or a PDF's line comes in one too, before it. [Untrusted Content](../../../../../../../../packages/untrusted/docs/more/libraries/runtime/untrusted.md) explains the fence.

### Turn it off

It is off until you add it. [`lnk agent tool remove file`](../../../../../../agent/docs/agents/README.md#tools) or the agent's
specification turns it off (`[tool.file] enabled = false`), or only
`file__glob` and `file__grep` (`find = false`), or the diffs (`diffs = false`): what it
turns off isn't listed to the model, and the answers carry no diff.

## Choose what it may write

Your agent writes only the folders its sandbox file's `[files]` lets
it. Link's is `work/`, `instructions/` and `skills/`, so it changes
its standing instructions and skills when asked, and `chats/` is never
written but by its harness. In this sandbox file the agent may add
skills, and with `instructions` only read, neither it nor its memory may
change its instructions.

```toml
# ~/Link/Safe/Agents/main/sandbox.toml
schema = 1
[files]
write = ["skills", "work"]
read = ["instructions", "chats"]
```

```sh
lnk agent start --sandbox sandbox.toml
```

### The whole of Home

`write = ["."]` is the whole files folder, its Home: an agent whose
files folder is a repository, as [`lnk agent start --run.files <repo>`](../../../../../../agent/docs/agents/README.md#start-your-agent) makes it, changes
any file of it.

### Its own folder

Its `instructions/`, `skills/` and `chats/` are in its own
folder beside Home (`~/Link/Agents/<agent>/agent`), named as Home's
folders are: beside `.`, `"instructions"` or `"skills"` lets it change
those, and `chats/` is never written but by its harness.

### Only its own conversation

Of `chats/`, each conversation reads only its own folder, as
`chats/telegram-42/`, never another conversation's. A stranger's
conversation reads none, nor does a call whose harness doesn't say
which conversation it's made in.

### What it tells the model

At the start of each session the tool tells the model that paths are
relative to the files folder, which folders it may write, and which
teams' folders it reaches and how. These come from the same grants the
sandbox holds to, so the model never has to guess an absolute path.

### Its sandbox

Its calls run in a sandbox made from that file, with no network, so the
sandbox holds to `[files]` whatever the tool is asked. One sandbox
serves a session's calls, started at its first call, and runs nothing
but File itself.

### Dot files a person's tools run

File never writes `.git`, `.envrc`, `.vscode` or `.idea`, at any depth
and in any case of their letters. Git, direnv and editors run what's in
them as you, outside any sandbox, so a write there is refused before
anything is made on the way. [The sandbox keeps them read only too](../../../../../../../security/sandbox/confine/docs/tooling/sandbox/wall/README.md#what-runs-outside-from-a-project).

### Links, sizes and pipes

File never follows a link, reads and writes text files up to 4 MiB,
and reads images and PDFs up to 2 MiB. An `edit` that would make a
file larger is refused before it's made, and a pipe is never read or
waited on. A write reaches the disk before it replaces the file, keeps
the file's mode, and never keeps a setuid, setgid or sticky bit.

### A file changed since it was read

A `write` or an `edit` of a file the session read or wrote is refused
when the file has changed since, by you or by another of the agent's
conversations, even where the text an `edit` replaces is still there.
The model reads it again, then writes. A `write` of a file the session
never read replaces what's there, and an `edit` of one changes it. An
`edit` is also refused if the file changes before the new text is in
place.

## Home in a bucket

Where your agent's files folder is kept in a bucket instead of on the
machine that runs it, on a cloud that runs agents only while they
answer, the same five tools reach it through the files bridge: a program
outside the sandbox that reaches the bucket and nothing else, each call
proved as your agent. The tool accesses Home remotely and can keep a
file’s text in session memory while paging a read.
The paths, the folders `[files]` lets it write and the sizes are the
same, held by the same check. A bucket holds no hidden files, so there
are none to read, write or skip.

### Its instructions and skills there

Such a cloud keeps your agent's `instructions/` and `skills/` in its
store, apart from Home, so the tools reach them through the storage
bridge instead: the program outside the sandbox that already keeps its
conversations, each call proved as your agent, so it reads and writes
your agent's files alone. The paths are the same as on a machine, and
your agent changes them only where `[files]` names them, as it does
there. A `glob` or `grep` finds them when its `path` names one, as
`skills`; with no `path` it looks in Home alone.

### Its teams' folders there

Its teams' folders are at `teams/<team>/` as on a machine, reached
as the cloud says your agent is on each team at that call, and another
account's team at `teams/<account>/<team>/`, where your agent reaches
what that team's owner let it. A `glob` or `grep` with no `path` looks
in its own files folder alone.

### A file changed meanwhile

A `write` uses the version that this session last read or wrote and
refuses a changed target. For a file the session has never seen, it
reads the current version first, or uses an absence condition for a new
file. An `edit` reads the current text and conditions its commit on
that read, preserving other text around the exact match. A concurrent
change between either operation’s read and commit is refused.

### Reading on

A `read` that goes on from a later line (`offset`) gives the rest of the file as
your agent read it from its first line, so all its pages are of one
version and the bucket is read once for them. Reading from the first
line again reads the file anew, as does a `read` after your agent
changes the file.

## Reach its team's folder

On a [team](../../../../../../agent/docs/agents/team/README.md#make-a-team), your agent's
paths reach the team's shared folder at `teams/<team>/`, as
`teams/research/drafts/report.md`. It reads and writes only the
folders of it its sandbox file grants, `[teams.<team>]`, which
[`lnk agent team add`](../../../../../../agent/docs/agents/team/README.md#manage-members) writes.

```sh
lnk agent team add research --member writer --write drafts --read sources
```

## Terminal Actions

[`lnk agent file`](../../../../../code-tool/docs/agents/tools/code/guide.md#terminal-actions) exposes File actions for the selected agent. Read an action’s JSON arguments from a file with `--request <file>`, or from standard input when the flag is omitted. The action uses the running agent’s configured tool and grants.

### Choose an Action

`lnk agent file read`, `lnk agent file write`, `lnk agent file edit`, `lnk agent file glob`, and `lnk agent file grep` use the corresponding calls described above. Use `--agent work` to select another agent and `--json` for machine-readable output.

## More about it

[Agents Security](../../../../../../agent/docs/agents/tools/use/security.md#run-and-file)
has the guarantees and gaps, and
[Decisions](more/decisions.md)
why it works this way. To work on its code, see [Developing Link
File](more/developing.md), and when it can't write or read a file,
[Troubleshooting](troubleshooting.md).
