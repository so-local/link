# Developing

`lnk-memory-tool` is Link Memory, the memory engine a Link agent uses by
default: an MCP server over stdio that searches, reads and lists the
agent's files and past conversations, keyless and local. How to use it,
and why it works the way it does:
[Link Memory](../README.md).

## What it reads and writes

It speaks Link's memory engine contract (`link_memory_tool_contract`),
whose `install`, `launch show` and `serve-many` its `actions.toml` lists
under `[implements]`, and any harness reaches it as a tool. It reads only its folder, writes only the
agent's notes there when Link names them (`--notes`), and never follows
a link out of it.

### What its command prints

Its `launch show` prints a `serve` line of its own. Its embedding model and
the most a search gives are the agent's specification's, from `LNK_MEMORY_SPEC`,
else `qwen3-embedding` and 6000 tokens, and `none` searches by words
only, with no model served. Where `LNK_MEMORY_NOTES` names the notes,
the line serves them too, as large as the specification's `notes`, else 4096
bytes, with `remember` among the allowed calls.

## Build and test

From the repository root, run `cargo build -p link-memory-tool`, `cargo test -p link-memory-tool`, then `cargo test -p link-memory-tool --test memory index::`.

### Its tests

`tests/agent.rs` drives it through Link's own [`lnk agent memory start`](../../../../../../../agent/docs/agents/state/files.md#lnksafesandboxestoolsmemory-idjson-600),
`tests/reads_conversations.rs` reads a conversation folder as Link
Harness keeps one, and `tests/notes.rs` writes the notes.

### PDF and Word text

PDF and Word text comes from Link Document Converter (`lnk-document-converter`), a
module of its own that memory runs as a child, with no secrets and with
its own resource limits, answering as [its
contract](../../../../../document-converter/contract/docs/agents/tools/memory/documents/more/contract.md) says; memory's build [compiles no document parser](../../../../../../../../../docs/more/developing/decisions.md#why-do-document-parsers-run-in-a-child-process).
Its command prints `programs: ["document-converter"]`, so its sandbox reads
that program where it's installed.

## What a model sees

Each answer's `content` is its JSON, fenced as third-party text. Its
`structuredContent` is the same answer without what the documents say:
a search's titles and snippets, a list's titles, and a document's links'
titles and targets are in the fenced text alone, so a client that gives
its model the structured content never shows them bare. A `read` keeps
its read receipt there but not its `text` or `title`, and a reader checks
the receipt against the `text` of the fenced JSON.

## Text Read Receipts

A document read includes a `read` receipt identifying its source snapshot
and selected byte span. Its text digest describes the returned lines,
after line-ending normalization. Lossy UTF-8 decoding is recorded as a
separate transformation and never credited as complete content. A requested
line count or missing continuation alone does not establish whole-source
coverage. The harness separately records any limit on delivery to its model.

## Many agents, one process

`lnk-memory-tool serve-many` serves every agent of one owner on the
machine, a session per connection, each answered from its own agent's
index alone.

### Its limits

Each of memory's own limits is in `src/limits.toml`, read once into
`Limits` (`src/limits.rs`) and checked by `the_limits_read`: the most
of a compact conversation's frames read (64 MiB), the most documents `links`
gives each way, an indexing batch, the write-ahead log, how often a
folder is walked, how long a full disk pauses indexing, and the
document converter's time and memory. What a call may ask for, the most results and tokens, is its
declaration's `maximum` in `actions.toml`, read from the calls it
renders, so it's never typed twice.

### Its Opinions

Those limits are decisions. What an agent's memory may spend is an
opinion, its specification's `[tool.memory]` keys (`Opinions`,
`src/opinions.rs`), each a `serve` flag (`--index-memory`, `--reserve`,
`--file-most`, `--threads`, `--deadline-ms`, `--dimensions`,
`--folders`, `--kept-off`) that `launch show` writes from the
specification, and Link's specification's where neither says
(`each_key_reaches_the_service_by_its_flag`).

### Session limits

A session's message is at most 1 MiB: past it, that session ends. At
most 256 sessions are served at once, and one more is refused, saying
so. A compact conversation's frames are read to 64 MiB, so a file an
agent wrote can't take the shared service's memory, and decoded only
back to the frame holding the last event indexed.

### When It Ends

The service ends 5 s after its last session ends, once no index it holds
has indexing left or documents' text being extracted, so what it began
is finished rather than begun again. A bound agent's index is written
back first. Embedding left holds up no end: the next service embeds from
the queue's front, where this one stopped.

### Opening each index

Each agent's index is opened at its first session, under
a lock of its own, so one agent slow to open holds up no other, and two
sessions arriving at once open it once. Opening reads nothing of the
index's contents: the session is welcomed once the file is open,
whether the index is empty, half built or complete.

### One Writer, Many Readers

Each index has one writer, a thread of its own (`serve::Writer`), which
does every write: it brings the index up to the folder in turns, and a
flush or a write-back takes the index between them. A session never
writes, and never takes the writer's lock.

### A Session's Reader

A session answers `initialize`, `tools/list` and `ping` itself, and
reads through a connection of its own (`Reader`), opened at its start.
SQLite's write-ahead log (WAL), a file beside the index where commits
go first, lets that connection read the last committed state while the
writer writes. So a call answers from what's indexed so far, and says
what isn't yet (`indexing`), never waiting for it.
`a_session_never_waits_on_the_writer` holds the writer in a long turn
and sees a new session's handshake and a `read` answered meanwhile.

### An Index With No File

An index given no file (`serve` without `--index`) is kept in a
temporary one, removed with it, since only a file can be read through a
second connection.

### Its embeddings

A search's words are embedded through the session's own reader, so a
model slow to answer holds up no other session. The service listens
before any model answers: a model served for it, or one memory starts
itself, is found in the background, and searches are by words until it
answers, as [its model's address](#its-models-address) says.
Passages are embedded by one thread for every agent the service serves,
as [One Thread for Every Agent](embedding.md#one-thread-for-every-agent)
says.

## Kept off the folder

Where an agent's machine keeps what memory reads off its folder (its
conversations, its notes or its memory, in SQLite, a bucket or a box's
store, as `~/.lnk/safe/agents/policy.toml`'s `[storage]` says), or its Home in a bucket
on a machine that runs agents only while they answer, the session's
first line binds the agent (`link_memory_tool_contract`'s `Hello` with
`Bound`): its owner, its key for this start, and its bridges. The
service then serves it by owner and ID, as no other owner's agent of
the same ID.

### Binding an agent

On a computer, `lnk agent memory connect` binds it so, with the key
this machine's keyring holds, and `lnk agent memory service` runs the
service with `--bridges <runtime>/lnk/agent/memory/<owner digest>` and
`--peer <its own user>`: the service runs in [`lnk sandbox run`](../../../../../../../../security/sandbox/docs/tooling/sandbox/guide.md#sandbox-guide), which on a Link box runs
its controller's work as a fresh worker user, so the user its agents'
sessions come from is named, not taken to be its own. The worker's view
of the service's folder shows its files as the controller's, so the
socket needs no shared group.

### Bridges Folder

The `--bridges` folder is in the runtime folder, where each agent's
bridge makes the socket memory reaches it at. It is named by a short
digest of the owner, never the owner's whole name, so every socket's
path in it fits a socket's limit.

### On a machine that runs agents only while they answer

Each agent, as it wakes, gets a memory of its own, `lnk-memory-tool
bridge`: an MCP server over stdio run as that wake's own user, so no
process holds two agents' memories.

- **Its bridges.** It reaches Home through the files bridge
  (`--socket`) and the collections `--kept <collection>=<place>` says
  are kept elsewhere through the storage bridge (`--store`), each call
  proved with the agent's key from the keyring (`--keyring`).
- **Keywords alone.** It searches by words, so it needs no embedding
  model.
- **Its index.** A cache, in `--cache`, of the copy beside Home: fetched
  when it starts, its `memory/` listed again then and once a minute,
  written back once a minute while it changes and when its input closes.
  The machine deletes the cache with the wake.
- **A team's memory.** `--team <id> --team-name <name>` serves the
  agent's session of that team's memory instead, written back only with
  `--team-writes`.
- **One-shot passes.** `--once` serves nothing: it lists what changed,
  writes the index back, removes its cache and ends. The machine runs
  one for an index wake and before the agent sleeps, while its key
  still proves.

### `Tree`

What memory indexes is behind one seam (`src/tree.rs`'s `Tree`):
a folder on the machine (`Folder`), with its conversations through the
storage bridge where they're kept elsewhere, or Home through the files
bridge and conversations through the storage bridge (`Bridged`), each
call proved as the agent.

### Listing a bridged tree

A bridged tree is listed when the agent is bound, and two seconds after
the files bridge says something under `memory/` changed
(`Control::Changed`), never on every call. A skill's file, never
indexed, is read by its path from either (`Tree::read_unindexed`), when
`read` finds nothing indexed there.

### Notes kept elsewhere

`remember` writes its notes through the storage bridge where its
memory collection is kept elsewhere (`notes.rs`'s `Notes::stored`), each a
write that names the version it read.

### A team's memory, bound

A session whose `Hello` names a team besides its binding is the
agent's session of that team's memory (`service/bridges.rs`), by the
team's ID: the team's folder's `memory/` through the files bridge as
the agent (`InTeam`), its index's copy beside the folder, cached at
`<folder>/<owner>/<agent ID>/teams/<team ID>/`. The bridge serves the
team's index only to a member granted all of the folder, so a member
granted part of it is refused at bind; only one writing all of it
(`writes`) writes the index back.

### A member that keeps no index

A session without `writes` or `semantic` embeds nothing: it
searches the copy beside the folder, and what changed there since, by
their words alone, never by meaning, since a search's meaning is a call
to the embedding model too.

### Paid team queries

`semantic` opts readers into the team's embeddings. A team proof appends
`:<team ID>:<files-bridge name>` to the binding header and binds both
values into its HMAC challenge. The endpoint resolves current membership,
folder access, opt-in and payer outside the engine, before each paid
attempt. The engine supplies neither payer nor spending allowance.

### Ended with its agent

It writes no notes and reads no conversations. The agent's `Flush`
writes its teams' indexes back with its own, and its `End` ends them; a
`Changed` naming the team lists that folder again.

## The index as a cache

The index is then a cache, mode 0600 (`<folder>/<owner>/<agent ID>/`
for an agent the service isn't named, else its own index file), of a
copy (`memory/src/cache.rs`): kept with the agent's `memory` collection
where that's elsewhere (`memory/link-memory/`, through the storage
bridge), else beside Home where Home is in a bucket.

### Its segments

The copy is one tar archive of the index's parts: its words' files
(`words/<name>`), oldest first, then its vectors (`codes` and `small`),
then its SQLite file (`index.sqlite`). The archive is cut into 8 MiB
segments named by their SHA-256, and a manifest naming them, written
last and only if it's still the version the cache was read from. What
didn't change since the last write back comes first, so its segments are
the same and aren't sent again.

### Streaming it back

A write back snapshots the index while no batch commits: its words'
files linked into a scratch folder, and its SQLite file copied with
SQLite's online backup. Its vectors are read as they are. The archive is
streamed a segment at a time (`index::Writer`), never read whole into
memory, and only the segments the copy lacks are sent. An index of
48 MiB with 16 MiB of vectors is written back holding under 24 MiB of
the service's memory
(`an_index_larger_than_memory_allows_is_written_back_streamed`).

### Fetching It Streamed

A fetch streams the copy back the same way (`index::open`), each part
into a folder aside, and a fetched index is found by its words and
embeds nothing again (`an_index_comes_back_with_its_words_and_vectors`).

### Its ceiling

An agent on a machine that runs agents only while they answer fetches
its index at each wake, so an index kept off the machine has a ceiling
(`[tool.memory] kept_off`, Link's specification sets `"2 GB"`), which
its parts count toward with its SQLite file. Past it, memory writes
nothing back, stops embedding, and searches by words, its status saying
why (`an_index_past_its_ceiling_isnt_written_back`).

### Fetching the copy

A cache with no copy fetches it, each segment checked against its hash
as it's unpacked. An entry the archive holds that no part names, or one
that isn't a plain file, refuses the copy. The copy is the owner's to
change, so its SQLite file is then checked whole (SQLite's
`quick_check`) and for memory's own schema alone (no table, index,
trigger or view of its own), and one that fails, or misses a segment,
is made again and written back. Every index is opened in SQLite's
defensive mode with its schema untrusted.

### Its Words, Checked

The copy's words are checked for memory's schema and each file against
its checksum. Words that fail are left out, and memory makes them again
from the files, as it does [after a crash between two
commits](index.md#words-and-sqlite-in-agreement).

### The machine's own bridges

The bridges a binding names must be the machine's own
(`--files-bridge`, `--storage-bridge`, or the agent's own
`<folder>/<agent>/storage.sock` in `--bridges <folder>`), else it's
refused before its key goes anywhere. A binding is served, even from a
cache already there, only once one of them takes its key, and a cache
is used only while it's of the copy there now.

### Writing it back

A changed index is written back at most once a minute, and at
`Control::Flush`, which the machine sends before the agent sleeps,
after embedding what's waiting for up to 20 seconds. A write back
refused for its version drops the cache, fetches the copy again and
indexes what it lacks.

### `Control::End`

`Control::End` drops the agent's key, and a session still open answers
nothing more; its cache stays, for the machine to remove. Every control
names the agent by owner and ID.

## Its model's address

`serve --embed <model>` reaches its model at `--embed-url`, which falls
back to `LNK_MEMORY_MODEL`, the variable the engine protocol gives an
engine its served model's address in. Each address means one thing:

- A URL: the model is served there, outside the sandbox, or is still
  starting there. Searches are by words until it answers a first
  request, asked again every half second in the background, so a
  service whose models are slow to start welcomes its sessions at once
  (`a_model_that_never_answers_holds_up_no_session`).
- Empty: a model was served for it, but no runtime here has it, so it
  searches by words.
- Left out: it serves the model itself, with [`lnk model serve`](../../../../../../../model/docs/agents/models/local.md#serve-one-on-this-machine).

`serve-many` reads each agent's serve line with that agent's own
environment from the agents file, never its own process's, so each
agent's address is its own (`link_actions::cli::under`).

## An embedding server for many owners

It's given as `--embed-url unix:<path>`, an OpenAI-style
`/v1/embeddings` on a Unix socket, each bound request carrying
`Authorization: Link-Binding <agent>:<proof>`. A bound endpoint must be
Unix: no binding credential is sent over HTTP.

### The proof

The proof is HMAC-SHA256 of `memory embedding binding` and the
binding's identity, made with its existing key
(`link_memory_tool_contract::embedding`). A shared engine proves each agent
separately. The proof lasts only for that binding and is held as a
redacted secret.

### The owner

The server resolves the owner from its Unix peer and the current
binding; memory sends no owner header.

## `tests/home.rs`

It runs all of the bridged memory against a stand-in files bridge over
an encrypted folder, and against a storage bridge over `lnk-sqlite-store`.
