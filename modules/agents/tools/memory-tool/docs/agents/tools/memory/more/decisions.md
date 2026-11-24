# Decisions

Why Link Memory is named, writes, indexes, is sandboxed and searches as
it does.

## Link Memory

What Link Memory is called, what it writes, and whose conversations it
remembers.

### What is Link Memory called?

Link Memory, the plain word, no brand. You type `lnk agent memory start
link`; its program is `lnk-memory-tool` and its module `memory-tool`,
mapped by the agent module’s `MEMORIES` catalogue.

### How does your agent reach its memory?

As a tool outside its sandbox, through the bridge every tool uses (`lnk
sandbox tool add`), at `$LNK_MEMORY`: one way in for every harness, the
calls logged and gated by the proxy. The tool is the agent's own,
`memory-<agent id>`, defined again at a start when its settings, the
engine or Link changed since, or the tool is gone, so it follows the
agent to another machine and a new release of the engine. The harness
gets one variable, whatever the agent's name.

### Does Link Memory write?

One file only: the agent's notes, `memory/notes.md`, through `remember`.
Its sandbox writes the folder `agent/memory/`, not the file alone, since
a file is replaced whole only by a rename, which takes its folder.
Everything else it searches, reads and lists, and never changes.

### Why traverse a parent without reading it?

The agent's private parent folder is opened for traversal without
reading its entries. Notes and chat copies are opened through their
own granted child folders, so saving notes needs no read access to the
rest of `agent/`.

### What does Link Memory add to a folder?

Only what's true of any folder of text: search by words and meaning,
filters, what changed since, links, a size limit. It assumes no kind of
note (people, projects, facts), runs no model over the notes, and
imports nothing. An engine built around one way of working plugs in by
the engine contract instead.

### What does a memory engine's contract fix?

Only how the engine starts: `install`, and `launch show <folder> <state>`
printing the command that serves MCP over stdio and which calls don't
ask you first (`link_memory_tool_contract`), with the agent's notes file in
its environment where it may write it. The bridge passes each server's
tools through, so no engine's names are translated, and a harness reads
each tool's description. Engines compete on what they serve, not on a
shape Link picked.

## Notes

How the agent's notes are written, kept, and given to it.

### Its notes

The notes are what an agent keeps in mind in every conversation, so
they're in its prompt, and a tool writes them rather than the harness,
so any harness with Link Memory takes notes the same way. They're one of
its instructions, so the sandbox file decides as it does for the rest:
where `[files]` keeps `instructions/` read only, Link names no notes
file to the engine, which then offers no `remember`.

### Never past its size

`remember` doesn't ask you first, as the agent's own edits to its
instructions don't, and it never writes past the specification's `[tool.memory]
notes`, 4 KB by Link's, so the notes stay a small part of every prompt.
There are two keys for the notes: `[tool.memory] notes`, what `remember`
writes up to, and `[instructions.max] notes`, what the prompt holds,
Link's 4 KB each. That is about 1,000 tokens in every prompt, and a file
edited by hand past the first is still cut by the second.

### Whose conversations can have the agent remember?

The owner's alone, where the harness is sent others' messages: a
teammate's, a team's, a stranger's and a subagent's conversations get
every tool but `remember`, so nobody else has the agent note something
for every conversation after. A chat the owner shared (`lnk agent
share`) counts as the owner's: its members can make the agent do all it
can, its memory included, and an owner who wants less shared makes a
second agent. A call that names no conversation is refused, as one can't
tell whose it is.

### What may a note hold?

A note enters every later prompt, so it is data, not an instruction.
Each note names the conversation it was saved in, the prompt labels the
notes as the agent's saved data, and only the owner's conversations see
them. `remember` refuses secret-shaped text as well as telling the model
not to save secrets, because a model sometimes saves what it was told to
keep, such as a locker code. The file is mode 0600, so no other user of
the machine reads it.

### How are an agent's notes kept?

As a markdown file, a line a note, which you read and edit as any other.
`remember` adds a line, or changes or forgets the one line holding a
piece of text it's given, so the model never rewrites the whole file and
loses a note by accident. It writes the file whole in one step, through
its folder's handle and never through a link, one writer at a time.

## What memory indexes

Which folders memory reads, and why it reads what it does.

### Which folder does memory read?

The agent's own: its Home, and its conversations in its own folder. One
folder shared by several agents would be a new decision about the
layout, and agents are kept apart everywhere else.

### Why does memory index only `memory/` and the conversations?

So what's searchable is what you put there, the same on every machine,
and a move never changes it. Indexing the whole files folder found
drafts, scratch files and a repository's code among your documents, and
paid to embed them. `memory/` is a folder of the layout for it;
everything else is still read by `file`, and a skill by its path through
memory's read.

### Why does memory read PDFs and Word documents?

Because the documents people keep are often PDFs: a tax report, a lease.
Memory extracts their text for its index, on every machine; the file
stays as it is, and `file` still reads text only. A document over 32
MiB is left out. Of a longer text, the first 2 MiB is indexed. Their
parsers run in the separate `lnk-document-converter` program, given no
environment, no key, and resource limits: the process serving memory holds every
bound agent's key, and a parser is where a crafted file would get in.

### Where does the converter run?

The service that serves many agents runs it in [`lnk sandbox run`](../../../../../../../../security/sandbox/docs/security/sandbox/guide.md#sandbox-guide), with
nothing granted but its own program, so it reaches no network and no
one's files. Extraction runs after the indexing that found a document,
on a thread of its own, each document's text written in its own short
transaction, so a slow parser never holds up indexing or a call.

### Why index a long conversation whole?

A long conversation is worth searching all the way back, and an agent
asked what was decided months ago looks there. Indexing only its newest
part lost the rest, and rewrote that part at every turn. Read from where
the last read ended, a turn costs what it says, so the whole history
fits.

### Where are conversations in memory's folder?

In `chats/`, where Link Harness keeps them, a folder per
conversation, its `conversation.md` the one indexed, never the event
files. A conversation kept compact or by a store driver has none on
disk: memory makes the same copy from its events, at the same path.
Memory follows the harness, and an end-to-end test holds the two
together. A search says which results are conversations, so a document
isn't lost among the chats about it.

## Memory's Index

What the index keeps, how it stays current, and where it's kept.

### Why doesn't the index keep the text?

The files are the source of truth. Keeping their text again, once in
the full-text table and once in every chunk, made the index 2.7 times
the corpus, and kept a second copy that could disagree. So the index
keeps each passage's place in its file and its hash, and a read, a
snippet or an embedding reads the file. Only what memory makes itself
and no file holds, a document's extracted text and a conversation's copy
made from its events, is kept in the index.

### Why doesn't a call refresh the index?

A call's cost should depend on what it asks, never on the folder's size.
Walking the folder before each call made a search over 150,000 files
wait for the walk. So a journal of changes, fed by a watch, the files
bridge's notices and the reconciling walk behind them, keeps the index
current in the background, and a call answers from what's indexed, as
[How Memory Stays Current](freshness.md) explains.

### How does an agent catch up on what changed?

`list {since}`, with `since` on `search` too: what was changed or first
seen after a time, each `new` or `changed`, and what was removed. The
index notes when it first saw each document, so a file copied in with an
old date is new all the same, and keeps the paths and titles of what was
deleted, the latest 10,000.

### What was there before

What was in the folder when memory first read it counts as there before,
not new, so turning memory on doesn't make everything news. `since`
takes a date, since a model writes one more reliably than a count of
seconds, or the seconds `list` gives. It holds for any folder of text,
as everything Link Memory adds does.

### Where is memory's index kept?

The index lives in the agent's `memory` collection at
`agent/memory/<engine>`, outside the harness's reach. It follows the
agent's folder and is included when [`lnk uninstall`](../../../../../../../../../docs/more/modules/guide.md#uninstall) removes agents.

### How is an index kept in a bucket?

The index's copy, one archive of its parts, is split into 8 MiB
segments named by their SHA-256, with a manifest written last (`link_store_contract::files::index`). A write
succeeds only against the version it read. Changed segments are
uploaded, and a concurrent writer receives a refusal and reads again,
preserving both writers' changes.

## Memory's sandbox

Why one process serves every agent's memory, and what its sandbox
reaches.

### Why does one process serve every agent's memory?

A session of memory on its own is a sandbox, the engine and its model
server: about 37 MB and six processes, started for each agent, and again
after it's been idle. Many agents on one machine would pay that many
times over, for one program reading folders.

### One engine for an owner

So an engine that can serve many folders (Link Memory) runs once for an
owner's agents, started by the first session that needs it and ending a
few seconds after its last, and each session is a small process that
passes its messages on. Each session is answered from its own agent's
folder, named by Link outside every sandbox, never by the agent.

### Told which agent

A team's shared tool runs one server for every member's session, as is,
which an agent's memory can't: each session reads a different agent's
folder, so the engine itself is told which agent each session is. One
owner's agents share it, never two owners': the folders its sandbox
holds together are all one person's, and no agent reaches the process
but through the session Link opened for it.

### Why does the memory engine run in a sandbox too?

It runs outside the agent's sandbox, as every tool does, but it needs
nothing a tool usually runs outside for: no logins, no desktop. So it
gets the least: it reads Home, the agent's conversations and skills in
its own folder, and its program, and writes its state folder and, for
the notes, `agent/memory/`. Link Memory already reads and writes nothing
else; the sandbox makes that true of any engine. It writes the folder
its notes are in, not the file alone: a file is replaced whole by
renaming a new one over it, which takes its folder.

### Why is memory's model served outside its sandbox?

Link Memory searches by meaning with a model [`lnk model serve`](../../../../../../../model/docs/agents/models/local.md#serve-one-on-this-machine) serves,
and running that from inside would need `lnk`, the models module and
what they run, so its sandbox read the whole folder its program is in.

### Started outside

Instead Link starts the model server for it outside, once for the
service its owner's agents share, and gives it the address, as a harness
is given its tools'. The address is a port chosen before the server
starts, so a runtime that comes up late is found there, its server
started again until it does, without the engine started again. Its
sandbox reads its own program and nothing beside it.

### How does Link Memory's sandbox reach its model?

On Linux, through its proxy's filtered path to a model
(`[localhost.linux] models`), at the one port its model is served on,
for inference only. The model keeps its address, so the engine calls it
as it would outside. The machine's whole network would reach every port
here, and a cloud's machine refuses it, since it reaches the metadata
service that hands out the machine's credentials. On macOS a sandbox
keeps a model's address only with the machine's network, so it has
`[network.macos] mode = "host"`. Either way it reads only its folder, and
writes only its state and its notes.

## Search

How memory searches by meaning, ranks, merges and keeps its vectors, and
how much a call gives.

### How does memory search by meaning?

With an embedding model on your machine, served by `lnk model serve`, so
memory stays keyless and free. A hosted model through the proxy would be
one more source, but memory is kept keyless. Each document's
[passages](index.md#what-the-index-holds) are embedded, each with its
document's title in front, read from the file as it's embedded.

### How does memory rank what it finds?

By its [words](index.md#search-by-words) and by its meaning, the two lists merged by their ranks. A document ranks by its
best passage. Within each list, documents within a tenth of the best
score around them match about as well, and the newest of them comes
first: a newer note beats an older one saying nearly the same, never one
that matches clearly better. A title from a file's name is embedded as
words (`passport-renewal.txt` as `passport renewal`), so it counts by
meaning as it does by words.

### Copies

A copy (all the same passages, or, from ten passages, nine in ten of
them, compared by their hashes, never by reading their text) comes once,
the others named in its `copies`. The first ranked copy is
kept; otherwise equally ranked documents prefer the shorter path. A
test of sixteen questions over a fixed folder (`tests/quality.rs`) holds
how many find their document first, so a change can't make search worse
unnoticed.

### How do words and meaning merge?

Reciprocal rank fusion: each ranks its best 50 documents, and a document
scores the sum of 1 / (60 + its rank) in each. There are no scores to
calibrate between the two, and a new model needs no retuning. There is
one `search`, no mode to pick: `matched` says why each hit came back.

### Why Tantivy for Search by Words?

SQLite's full-text search scores every passage holding a word before it
can rank one. Over a generated corpus of 45,000 files of 5,000 lines
(11 GB), a common word took 6.1 s and a question of six words 1.0 s, and
both grew a little faster than the corpus. Tantivy, a search library
widely used in Rust, finds the best passages with block-max pruning
instead, skipping the blocks that can't beat them. The rest of the index
stays in SQLite: documents, passages, links, counters and the embedding
queue.

### How Fast Is Search by Words?

On a 4-processor Linux machine with 15 GB of memory, release build, page
cache warm, the median of 7 searches asking for 10 results:

| Query | SQLite, 45,000 files | Tantivy, 3,000 (0.74 GB) | 15,000 (3.7 GB) | 45,000 (11.1 GB) |
|---|---|---|---|---|
| Rare word | 28 ms | 1.8 ms | 1.2 ms | 2.0 ms |
| Middling word | 193 ms | 0.9 ms | 1.3 ms | 2.2 ms |
| Common word | 6.1 s | 1.2 ms | 1.7 ms | 2.9 ms |
| Two common words | 6.2 s | 1.5 ms | 2.6 ms | 4.5 ms |
| Two middling words | 48 ms | 1.2 ms | 2.3 ms | 3.8 ms |
| Middling and rare words | 16 ms | 0.9 ms | 1.5 ms | 2.8 ms |
| Question of six words | 1.0 s | 1.7 ms | 4.1 ms | 17 ms |

A common word's search scored about 9,000 passages at each size, while
the passages holding it grew fifteenfold, and no search reached its
300 ms deadline. The
index, words included, was 0.22 of the corpus, built at 16 MB/s.

## Embedding

How vectors are kept and embedded, with which model, and who pays for a
team's.

### Why binary codes, not a graph index?

Each passage keeps a binary code and a small vector in [two files beside
the index](embedding.md#how-vectors-are-kept), compared in Rust: every
vector of 150,000 long files held in memory whole would take 127 GB.
Codes need no training and no rebuilding, take a sixteenth of the space,
and scanning them all is simple to explain and fast enough: a gigabyte
of codes is counted through in well under a second. Only the few hundred
nearest are rescored.

### Why does embedding run in the background?

So the first index doesn't hold up a search: what's embedded is searched
by meaning, the rest by words, and the reply says how far it got. What's
embedded first is the [queue's](embedding.md) front. The embedding
thread reads the queue through a connection of its own to the index's
file, and the vectors stay in their files from one session to the next,
so only what changed is embedded again.

### Which embedding model does memory ask for?

`qwen3-embedding`, Apache-2.0 and many languages, over the smaller
`nomic-embed-text`. A model under terms that aren't permissive isn't
asked for.

### Another model

The agent's specification can name another, or `none` to search by words only. A
specification that names something that isn't a model's name is refused, so a
typo never runs another model than asked.

### Why does a team's member search by words unless the team says?

A search by meaning embeds its query, a call someone pays for, and a
team's embeddings are billed to whoever pays for the team's memory. So
a member that doesn't keep the team's index embeds nothing unless [the
team's specification](../../../../../../../agent/contract/team/docs/agents/more/interface/contract/team.md#team-settings) turns search by meaning on, with a cap a day its members
share.
Naming a payer grants no spending: the service hosting the team checks
that account's consent before each paid call, outside the engine.

## Search Results

How a search bounds its answer and lets a caller follow related documents.

### How much does a memory call give?

At most about 6,000 tokens, four characters each, unless the call asks
for another size: under the 32 KiB Link Harness keeps of a tool's
answer, so an answer arrives whole rather than cut mid-way. `search`
gives whole results and `read` whole lines, at least one, and each says
where it stopped (`next`), so the next call carries on from there rather
than starting again. A search's and a list's `next` is a [page](index.md#pages)
that names where it stopped, so the next call never redoes the pages
before.

### What's cut

Only a single line longer than the size is cut, and the read says so
(`cut`). `list` and `links` keep the same size, leaving out whole
entries that don't fit, and a title is at most 200 characters, so no one
document fills an answer.

### How does memory follow links?

`links {path}` gives what a document links to and what links to it, from
markdown links and wiki links, the two ways notes link in a folder of
text. The index keeps each link as written, resolved against the folder,
and matches it to documents when asked, so a link to a note made later
starts working without reindexing.

### Wiki links

A wiki link names a document by its path or its file's name; several of
that name, the shortest path, as notes apps do. Nothing is followed on
disk: a link only names an indexed document, and one to the web or out
of the folder is no link. It is a fourth read-only call, so it never
asks you first. A document keeps its first 1,000 links, and none longer
than 512 bytes.
