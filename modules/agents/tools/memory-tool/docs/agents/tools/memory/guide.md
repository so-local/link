# Memory Guide

Start Link Memory, add searchable documents, and choose whether searches match words or meaning. Its saved notes and index remain separate from the files and conversations they describe.

```sh
lnk agent memory start
lnk agent memory
```

## Start it

Your agent's first [`lnk agent start`](../../../../../../agent/docs/agents/README.md#start-your-agent) offers it, and `lnk agent tool add
memory` does the same as `memory start`. `start` restarts your agent if
it's running as a service; one running in a terminal keeps what it had
until you start it again there, and `start` says so. It says where the
notes it keeps are, and `--agent <name>` picks another agent.

### What it needs

You need an agent, and [`lnk agent memory start`](../../../../../../agent/docs/agents/state/files.md#lnksafesandboxestoolsmemory-idjson-600) adds Link Memory if
it's missing. Searching by meaning also takes an embedding model, a
download of a few hundred megabytes. Nothing costs anything, because it
all runs on your computer.

## Inspect Memory

`lnk agent memory show` reports the selected engine and its endpoint, and is the default when you run `lnk agent memory`. `lnk agent memory show` reads retained notes with their sources through
its action namespace. In Link Harness, `/memory:show` reads those notes. Use `--agent work` to inspect another agent. These reads do not enable or disable the engine.

## Search by meaning

An embedding model lets a question find a note that shares none of its
words.

```sh
lnk model install qwen3-embedding
```

Then "what did I plant last spring" finds "tomatoes, April, the back
bed". Nothing needs a restart: memory finds the model the next time it
starts, once your agents have left it idle.

### By words only

Without the model, a search is by words only, and says so and how to
add the model. To search by words only on purpose, set
`[tool.memory] model = "none"` in the agent's
[specification](../../../../../../agent/docs/config/specs/agent.md).

### The first time

The first time, Link Memory embeds your files in the background:
conversations and notes first, then the text of PDFs and Word documents,
each newest first. Meanwhile a search is by meaning over what's done and by words
over the rest, and says how far it got. After that it embeds only what's
new or changed, on your computer's CPU or GPU, at no cost.

## What it finds

Your agent can search what's in its files folder's `memory/`, and its
past conversations. Nothing else of the folder is searched: your agent
reads the rest with `file`, but memory doesn't index it, so a search
finds only what you chose to give it, on every machine alike. To give
it a note or a document, such as `garden.md` or a PDF of last year's tax
return, copy the file into `~/Link/Agents/main/home/memory/` with your
file manager or any copy command.

### Marked as kept text

What a search, a read, a listing or a document's links give comes inside a fence, an opening `<untrusted ...>` line and a closing line with the same random ID, which tells your agent's model that it's data to read and never your instructions, since anyone may have written it. A note's `remember` answer says only what it did. [Untrusted Content](../../../../../../../../packages/untrusted/docs/more/libraries/runtime/untrusted.md) explains the fence.

### What it reads

That means markdown, text, JSON, CSV, YAML, HTML and the like, up to
[`[tool.memory] file`](#choose-what-it-spends) each (16 MB unless you
choose otherwise); PDFs (`.pdf`) and Word documents (`.docx`) up to 32 MiB,
the first 2 MiB of their extracted text, once you add [the module that reads them](../../../../document-converter/docs/agents/tools/memory/documents.md);
and in `chats/` each conversation's `conversation.md`.

### A long conversation

A conversation is indexed whole, however long it grows, so a search
finds a word from its first turn as one from its last. Memory keeps
where it left off, the last byte of a copy kept as a file or the last
event of one it makes, and [reads only what came after](more/index.md#conversations):
a turn costs memory what the turn says.

### Documents in the background

A PDF or Word document is found by its name at once. Its text is
extracted after the indexing that found it, a few documents at a time,
so a search never waits on a parser. Its words are found once extraction
finishes.

### What it finds by name

A PDF or document with no text to read, such as a scan, or with the
module not added, is found by its name alone, and a search's reply says
why. Each result says whether it's a `file` or a `conversation`. What
you add, change or delete is indexed in the background, with nothing to
rebuild. A call never waits for it: while something isn't indexed yet,
a search's reply says so (`indexing`).

### How it sees a change

On Linux memory watches its folder, so a search finds a file you write
within a couple of seconds, and walks it now and then for what a watch
misses, as [How Memory Stays Current](more/freshness.md) explains. On
macOS it walks the folder each minute, so a change is found within a
minute. It knows a file changed by its size and its modification time. An
edit that keeps a file's size within the clock's step, as on a file
system that keeps time to the second, isn't seen until the file changes
again. Touch the file (`touch ~/Link/Agents/main/home/memory/garden.md`)
to have it read again.

### A conversation's copy

A conversation's copy holds what was said up to the agent's last
answer, without the tools' calls and answers. So a search finds what
was said earlier in the chat you're in, and never the question it's
searching for.

### A conversation with no copy on disk

A conversation Link Harness keeps compact, or in a store driver
(`[storage] conversations` in its specification), has no `conversation.md` on
disk: memory makes the same copy itself, from its messages, found at
the path the file would have. It's made again when the conversation
grows. For a driver's, memory runs only the driver that
`[storage] conversations` names, which Link passes it, whatever a file
in the conversation's folder says. Its sandbox reads that driver's
program, as Link Harness's does.

### Kept off its folder

Where your machine keeps its conversations, its instructions or its
memory somewhere other than its folder (in SQLite, a bucket, or a store
off the machine, as `~/.lnk/safe/agents/policy.toml`'s `[storage]` says), memory reads
and writes them through the agent's storage bridge, as the agent, while
it runs: its conversations, its notes, and a copy of its index kept
with its memory, so another machine it moves to finds the index it
left.

## Let it take notes

In Link Harness, `/memory:remember` invokes the notes action with its JSON
arguments. The conversation and write restrictions below still apply.
Your agent notes for itself what it should keep in mind in every
conversation, and Link Harness puts its notes in its prompt. The notes
are a file you can read and edit, `~/Link/Agents/main/agent/memory/notes.md`.

```markdown
- My sister is Maya.
```

Tell it "remember that my sister is Maya" and it calls `remember`, which
adds a line like that one to `memory/notes.md`, changes one or forgets one. The
notes stay under 4 KB, so they fit in every prompt: past that,
`remember` refuses and your agent changes or forgets a note first.
`[tool.memory] notes` in its
[specification](../../../../../../agent/docs/config/specs/agent.md) sets
how large.

### What a note may hold

Each note says the conversation it was saved in. `remember` refuses
text that looks like a secret, such as a password, a key, a token or a
code beside its digits, and tells the model never to save one. The notes
file is readable by you alone (mode 0600).

### Who sees the notes

Link Harness gives the notes only to your own conversations, labelled
as data the agent saved and never as your instructions. A teammate's,
a team's, a stranger's, a webhook's and a subagent's conversations
never see them, since a note saved from text someone else wrote could
otherwise steer every later answer.

### Notes across conversations

Conversation history and saved notes are separate. A fact repeated in
the CLI may only be in that conversation's transcript. Saving it with
`remember` makes it available to the same agent in a fresh CLI chat or
on Discord, Slack and other connections. Deleting a conversation does
not delete these notes. The agent is instructed to save stable owner
facts during the turn and confirm saving only after the call succeeds;
the model still has to make that call. Check `notes.md` to verify it.

### What it writes

`remember` writes nothing else, and where its sandbox file keeps
`instructions/` read only, there is no `remember`.

## How your agent calls it

Your agent searches, reads and lists its memory through four calls, and
none of them changes a file. A fifth, `remember`, writes its notes and
nothing else.

### `search` and `read`

In Link Harness, `/memory:search` and `/memory:read` invoke these actions
with the corresponding JSON arguments and memory grants.
`search` finds documents by their words and their meaning, best first,
each with a snippet, and `read` gives a document, whole or some lines
of it.

### `list`

In Link Harness, `/memory:list` invokes this listing with its JSON options.
`list` gives the documents, the most recently changed first. With
`since` it gives what changed, so an agent catches up at the start of a
session.

### `links`

In Link Harness, `/memory:links` invokes this action with its JSON document
selection.
`links` gives what a document links to and what links to it, from
markdown links and wiki links (`[[Other note]]`), and what it links to
that memory doesn't hold: the first 200 of each.

### How much a call gives

Each call gives at most about 6,000 tokens and says where it stopped,
so the next call can carry on from there. A `search` or `list` answer
that stops early gives `next`, which the following call passes as
`page`, and a `links` answer gives a `next` passed as `skip`. A
`search` gives `next` within its first 50 results, and past them says
to narrow the search. A search that stopped ranking at its deadline says
so (`stopped`).

## Choose What It Spends

People run memory on a laptop, on a server with a large disk, or kept
off the machine, so what it may spend is yours to set, in the agent's
specification under `[tool.memory]`, each key also a flag
(`--tool.memory.<key>`). Link's specification sets each, and how memory
spends them is in [The Index](more/index.md).

### `memory`

The most memory's service holds of its own for the agent's index,
besides the pages of files it maps. Link's specification sets
`"512 MB"`.

### `reserve`

The disk left free: below it indexing pauses, calls go on answering,
and it resumes once there's room. Link's specification sets `"1 GB"`.

### `file`

The largest file in the agent's folders memory indexes. Link's
specification sets `"16 MB"`.

### `threads`

The threads indexing takes, so a laptop isn't taken over. Link's
specification sets 1.

### `folders`

The agent's folders memory indexes: `memory`, `chats` or both, as
Link's specification sets.

### `deadline`

How long a search ranks by words before it answers with the best of
what it has, saying so. Link's specification sets `"300ms"`.

### `dimensions`

How many of the embedding model's dimensions each vector keeps: fewer
cost less memory and disk, more rank a little better. Link's
specification sets 256.

### `kept_off`

The largest index kept off the machine, fetched at each wake where an
agent runs only while it answers: past it, the index isn't written back
and search is by words, as memory's status says. Link's specification
sets `"2 GB"`.

## In each harness

[Link Harness calls memory by itself](../../../../../../harnesses/link/docs/agents/harnesses/link/guide.md#give-it-memory-and-tools), as `memory__search` and the rest.
A harness from outside Link finds it in its own MCP settings, as the
server `memory`, added at each start.

### A skill's file

Link Harness reads [a skill's `SKILL.md`](../../../../../../harnesses/link/docs/agents/harnesses/link/guide.md#give-it-skills) with `read` when a task needs
it,
by its path, `skills/<name>/SKILL.md`. Memory reads a text file in
`skills/` straight from the folder, never through a link, but never
indexes it, so no search or listing gives one. A skill kept in
`memory/` is listed by its folder, since every skill's file is
`SKILL.md`.

### Through its proxy

Your agent reaches it only through its proxy, so not with
`--network.mode host` or without `[internet]`. To try it as your agent
would, the first command prints the address your agent calls, and the
second lists every call.

```sh
lnk agent harness run --harness link -- sh -c 'echo $LNK_MEMORY'
lnk sandbox log show
```

## Stop Memory

`lnk agent memory stop` disables the selected engine and removes its tool definition. Its files, notes, and saved index stay in place so you can enable the engine again later. A running agent receives tool changes through the normal restart workflow.

```sh
lnk agent memory stop
```

## More about it

[Agents Security](../../../../../../agent/docs/agents/tools/use/security.md#memory) has the
guarantees and gaps, and
[Agents Decisions](more/decisions.md#how-does-your-agent-reach-its-memory)
why it works this way. To work on its code, see
[Developing Link Memory](more/developing.md), and when it doesn't find a file or
search by meaning, [Troubleshooting](troubleshooting.md).
