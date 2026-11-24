# Decisions

Why Link Harness works the way it does. Link's own decisions about
harnesses are on its [Agents Decisions](../../../../../../agent/docs/agents/more/decisions.md) page.

## Design

What Link Harness is called, how it's judged, and the seams it's built
on, which let any process on any machine take an agent's next turn.

### What is Link Harness called?

Link Harness, the plain word, no brand. You type [`lnk agent harness set link`](../../../../../../agent/docs/agents/harnesses/use/README.md#switch-in-one-command);
its program is `lnk-harness` and its module `harness`.
`KNOWN` (harness settings) maps the name you type to the module, and an
adapter from outside Link stays `lnk-<name>`, found by its own name.

### How is Link Harness judged?

By what each agent costs, idle and per message, as Link measures it, with [`lnk measure`](../../../../../../../tooling/measure/docs/tooling/measure/README.md#measure). So it gets lighter one bottleneck at a time, the
largest first.

### How are Link Harness's seams built?

For the densest deployment, run at the safest: the agent is always an
argument of a turn, and each request names it; settings are read at
each turn; every path comes from the agent through one function; and
one turn runs at a time per conversation, a lock in the process. Each
uses the same store and turn boundary whether one process serves one
agent or several.

### Does Link Harness need the harness hub?

No. Link Harness runs on Link's own services alone: the model proxy,
the sandbox, the shared bridge that carries connections, the store and
the tools. The [hub](../../../../../portability/hub/docs/agents/harnesses/more/protocol/hub.md)
is only for an agent that runs on, or moves between, another harness, so
an agent that is Link's code from end to end pays nothing for it. Link
Harness still answers the Agent State Protocol's (ASP's) state methods, so another harness can take
an agent from it and give it back.

### What does Link Harness keep in memory?

Nothing that matters when its process dies. It's a small router: each
turn reads what it needs through its store, and writes there. The
events are the truth, each number written once. Everything else (where
the window starts, the copy for people, its settings as last read, each
tool's list of calls) is derived, and made again by a process without
it. So any process on any machine can take the next turn.

## Agents

How one process of Link Harness answers for many agents, or starts
ahead for one it doesn't know yet.

### Can one Link Harness serve several agents?

Yes, of one owner, where the machine says so (`serve --agents <file>`,
the contract's `shared`). Each agent is answered on its own port, as
itself, from its own home, files and tools, the ones it would have had
alone; a request naming another agent needs that agent's key.

#### What it costs

Since it keeps nothing that matters in memory, one process for many
costs only what each turn reads, and a restart (an agent joining or
leaving) is finished like any other. Stopping for one, it takes no new
connection, and answers on each it has until its client closes it: a
bridge opens one for each message, so a message asked just before the
stop is allowed to finish within the shutdown drain. Link runs it so only where
`~/.lnk/safe/agents/policy.toml` says `shared_harness`, since the agents' folders then
share a sandbox.

#### Stopped by a signal

Sent SIGINT, SIGTERM or SIGHUP, which `lnk` passes on to it, it stops
as for one agent leaving, whether it serves one agent or several: it
takes no new connection and drains active connections and answers for
up to 30 seconds, then exits. Unfinished turns use the normal crash
recovery path on restart.

#### A turn cut off by a stop

A turn can still be running once every answer is given, such as a
summary written after its answer. The stop lets go of that turn's lease
before it exits. The next process to take the conversation then finishes
the turn at once, instead of waiting for the lease to lapse (`[turns]
lapse`, 60 seconds in Link's specification) as it would for a crash.

#### An agent asleep

An agent stopped for being idle stays one it serves, so its sleeping
and waking restart nothing, and with every agent asleep, it ends.

### When does Link Harness answer?

Only once its tools are listed, so its first turn's prompt is the one
[`lnk agent show --instructions`](../../../../../../agent/docs/agents/models/agent/README.md#set-its-model-and-how-hard-it-thinks) prints. It takes its port as it starts,
so a port already held fails the start at once, but it listens on the
port only once its tools are listed.

#### Refused, not left waiting

A connection before then is refused at once rather than accepted and
left unanswered. So its health check, and `lnk agent show`, learn at
once that it isn't answering yet, even on a machine too busy to list
its tools quickly.

### Why can Link Harness start blank?

So a machine that runs an agent only while it answers needn't count a
start in the time to answer. It keeps units started ahead, each holding
nothing of anyone's: no settings, no key, no port (`serve --control
<control>`, the contract's `blank`).

#### Bound once

Bound to an agent by one line on its control socket, a unit answers as
that agent at once, with a key made for that binding alone, the
settings it was bound with kept in memory, and its conversations
through the store bridge it was given. It's bound once and never
reused: there's no unbinding, so nothing of one agent's can be left for
the next, and the process ends when its agent sleeps.

## What it keeps and reads

What goes through its store, where its prompt's sections come from, and
what becomes of an answer nobody took.

### What becomes of an answer nobody was there to take?

It's kept for the bridge, as a crashed turn's answer is. A turn runs to
its end even when the caller goes, since its calls were made and its
answer is in the conversation either way. A caller goes when Link's
bridge stops mid-answer, and where one bridge carries every agent, one
stop cuts every agent's turn at once.

#### Handed over once

So an answer whose caller was gone before it was told it is handed over
at `_link/recover` with the crashed turns', and the bridge, asking
every half minute while the harness runs, sends it to the chat once.
The message is never asked again, so no call is made twice.

### What goes through Link Harness's store?

Everything it reads and writes: its settings (`config.json`, written by
`configure` through the store too), its instructions, its conversations
and where each one's window starts. One seam per resource, keyed by
agent: the store has one implementation, the agent's folders on disk,
which keeps each conversation's events [one of three ways](#how-does-link-harness-keep-a-conversation), and the code is
the same on every machine.

#### The workspace and memory

The workspace, the folder an agent's tools work in, is the agent's
files folder, named by the same one function. Link Memory reads a log's
copy on disk itself, and makes its own of a conversation kept any other
way, reading its events as Link Harness does: the compact frames, or
the driver.

### Where does Link Harness take its instructions from?

The specification's `[instructions] source` chooses the whole prompt. `"link"`,
the default, uses the section assembly below. A Markdown file supplies
the prompt by itself and `"none"` leaves it empty. Assembly is a small
optional function within the harness, since selecting a file needs no
separate package or module.

#### Plain Markdown

A file is read through the same storage seam at each turn, with no
added headings, sections, template expansion or message timestamps.
It has no model section to swap when a fallback answers. Section
settings remain in the specification for switching back to `"link"`, and a
missing file never falls back to Link's assembly.

#### Link's assembly

From sections, each with an owner and a most, composed in the order of
how often each changes, least first: Link's base, yours for the model
it runs on, what its tools say of themselves, the environment (its
files folder and connections, at its start), the repository's, its
skills, who it is and who you are, whatever the task
(`instructions/persona.md` and `owner.md`), your standing instructions
(`instructions/instruction.md`, read at each turn, a file like any
other), its notes, and now.

#### Why that order

A provider caches a prompt up to its first change, so what changes
least goes first, and the prompt stays the same each turn: the date,
the time, the connection and who's speaking change every message, so
they head each message instead, kept with it so it reads the same at
each later turn.

#### The time and who's speaking

The time is the machine's local time, where a personal agent's owner
is; the scheduler's zone is its own key. Who's speaking is shown only
for the owner, since a message from others already says who, quoted.

#### Each one's source and size

Each one's source and size are opinions, the specification's `[instructions]`;
every section can be replaced, the base included. Everything, a file of
yours too, is read through the store, as conversations are, never
through a link.

#### The repository's

The repository's are `AGENTS.md`, the standard most coding agents read,
else `CLAUDE.md` for a repository written for Claude Code: one per
folder, so a `CLAUDE.md` that only points at `AGENTS.md` isn't read
twice.

### Where are a model's own instructions?

In the agent's files, `instructions/models/<maker>/<model>/instruction.md`,
beside the standing instructions, never in its specification. Instructions are
long and formatted, so they're markdown files you edit, and a specification only
names a file or `link` or `none`, never holds the words.

#### How a model is named

A model is named as OpenRouter names it, its maker then its name
(`anthropic/claude-opus-5-5`, `openai/gpt-6-luna`), since it names
every hosted model and models through it already are; one on a runtime
here has no maker Link can know, so it goes under the runtime's name
(`ollama/qwen3:8b`).

#### Two names for one model

The same model reached two ways can have two names (Anthropic's
`claude-opus-5-5` and OpenRouter's `anthropic/claude-opus-5.5`): each
is its provider's, never guessed between.

#### Why it comes second

The section comes second, after the base: it changes only when the
model does, and a model changed is a new cache anyway. A fallback model
is sent its own file in the first's place, the rest of the prompt the
same.

### Where does the tools section come from?

From the tools: MCP's `initialize` answer has a server's own
`instructions`, how its tools work together, which Link's tools write
(the scheduler's subagents and teammates, memory's notes) and any MCP
server may. The text comes from the tools you added, as their
descriptions already did.

#### How it's listed

Link Harness keeps them with each tool's listing, at most 2 KB each,
and lists them under each tool's name, sorted, for the tools the turn
offers: a conversation narrowed to no tool of one has none of its
words. Kept beside the listing, a tool that fails to start in a new
process is still described as it was, so the prompt, and the cache,
hold.

### Where do Link Harness's notes come from?

From its memory: `remember`, a tool of Link Memory's, writes
`memory/notes.md`, a line a note, and the composer reads the file into
the seventh section at each turn. A tool rather than code in the
harness, as every tool is, so any harness with Link Memory takes notes
the same way.

#### Late and small

The section comes late, so a new note changes the prompt from there on
and the provider's cache holds up to it. The notes are held small, 4 KB
by Link's specification, since they're in every prompt: past it, `remember`
refuses and says to change or forget a note first, as Hermes Agent
holds its memory file to a limit for the same reason.

#### An empty file

The section is the file under "# Your notes", and none when the file is
empty, so an empty file costs no tokens.

### How does Link Harness use a skill?

The prompt lists each skill's name and description. The model reads its
instructions through Link Memory's `read` when needed, following the Agent
Skills format. Skill metadata is read through the store at each turn and
sorted by name, keeping the prompt stable. Instructions are cached until
the skill file changes. A skill missing its matching name or description
is omitted, and prompt limits remove whole skills rather than fragments.

## Conversations

How a conversation is kept, how Link Harness tells which way, and why
it stays in files by default.

### How does Link Harness keep a conversation?

A conversation is an append-only event log, selected by the agent specification's
`[storage] conversations`: `log`, `compact` or a store driver. A log is
`events.jsonl`, with one JSON event per line. A lock serializes writers;
an existing sequence number is refused, and an incomplete final line is
removed before appending.

#### What its events retain

Events retain text, time, model, provider, response ID, tokens, latency
and provider-reported cost. Thinking accompanies model calls but is not
shown. [A Markdown copy](#what-does-a-conversations-copy-hold) supports people and memory. Storage is
keyed by agent and conversation so backups, moves and harness switches
read the same record.

### Why is how a conversation is kept the specification's to say?

Because people want different things of it. One opens the folder and
reads it; another wants it as small as it can be, and never reads it by
hand; a third keeps it in a database, off the machine. So it's an
opinion, a key of the specification (`[storage] conversations`), read by Link
harness from its settings as every opinion is; a harness from outside
Link keeps its own however it does, so the key is Link Harness's, and
the specification says so. The rest of what Link Harness keeps would be more
keys under `[storage]`.

### How does Link Harness know how a conversation is kept?

From its folder: an empty `store.<driver>` says a driver keeps it,
`events.zst` that it's compact, and `events.jsonl` that it's a log.
A folder without a recognized marker has no stored conversation yet. The specification says only how a new one starts,
so changing it moves nothing: each conversation is read where it is.

#### Moving the old ones

[`lnk agent chat export --format`](../../../../../../agent/docs/agents/chat/README.md#manage-history), by hand and with the agent stopped,
moves the old ones: each is written the new way in a folder beside it,
put in its place in one rename, what else it held (its tools, an answer
still to send) moved along, and the old way's rows forgotten. Never on
its own: a key changed by mistake shouldn't rewrite every conversation.

### What is a compact conversation?

The same events as the log, a line of JSON each, in zstd frames
appended to `events.zst`, so a write never rewrites what's there; the
events since the last frame are lines of `events.jsonl`, and every 32
of them (or 1 MB) become the next frame, compressed hard (level 19),
since a frame is written once per many events.

#### Its lock and its frames

The tail's lock is the conversation's: a reader holds it shared, so it
never sees a frame written and its events still in the tail. A frame is
whole on its own, so one cut short by a writer that stopped isn't read,
and the next fold cuts it off; its events are still in the tail,
emptied only once the frame is on the disk, and an event found in both
is read and framed once.

#### Its Fold, Off the Turn

The append that brings the tail to a frame's worth never compresses
it. It hands the fold to a thread of its own, which reads the tail
under its lock as a reader does, then compresses it with no lock held.
So the turn's next appends and reads go on while the frame is made.

#### Its Frame, Written Under the Lock

The fold then takes the lock as a writer does, for as long as writing
the frame takes. It writes the frame only if the tail still starts with
what it compressed and no other fold wrote one meanwhile. What came in
meanwhile, a few events, becomes a small frame after it, and only then
is the tail emptied. Cutting the compressed lines off the tail's front
would leave a moment when a writer stopped there loses the rest.

#### A Fold That Never Ran

A program waits for its folds before it ends, for at most 30 seconds.
One that ended first, killed say, left the tail whole: its events are
read from there, and the next append hands the fold on again. So does
an append whose fold's thread can't start, which says so and still
reports its event written. Only a tail of four frames' worth, its
folds lost each time, is folded by the append that reaches it. A frame
holds more than 32 events when its fold ran late.

#### Its first frame

The first frame is one zstd skips, naming the thread, which its
folder's name doesn't say whole (`telegram-42` for `telegram:42`).

#### What compression costs

Compression reduces disk use at the cost of work during writes and
reads. A compact conversation has no frame index, so a read takes in
the compressed file whole, but decodes its frames newest first and only
back to the one holding the first event it asks for. A turn decodes
about its window, however long the conversation. A fold compresses up
to 1 MB at level 19 with no lock held, so neither the turn's appends
nor its reads wait for it.

### Why no copy for people beside a compact conversation?

A copy grows as the conversation does, and as plain text, so it would
take most of what compact saves. [`lnk agent chat show <thread>`](../../../../../../agent/docs/agents/chat/README.md#talk-to-it-here)
prints it from the events instead, and Link Memory makes its own.

### What is a store driver?

A module, `lnk-<name>`, that keeps conversations for Link Harness: it
writes an event only if its number is free and the next, reads from a
number, and keeps the lease that says a turn runs. That's all a
conversation needs, so any process on any machine sharing the driver's
store can take an agent's next turn.

#### SQLite

Link's first is SQLite (`lnk-sqlite-store`), local: every conversation of the
agent in one file, `chats/events.sqlite`, so it moves with the
agent; each write is a transaction that takes the file's lock first.

#### Behind the store bridge

The driver runs outside Link Harness's sandbox, behind the agent's
store bridge, which Link runs beside the agent: Link Harness asks it
for each call, proved with the agent's key, so whatever the driver
needs to reach its store never enters the sandbox, and Link Harness
neither knows nor cares which driver it is.

#### Another driver, and what the folder keeps

A conversation kept by another driver than the specification's runs that driver
in Link Harness's sandbox, as memory does. A driver that serves is kept
running between calls, at most two idle of it, each stopped once idle
for a minute, so a turn's calls cost one process. A call is stopped past
30 seconds. A conversation's folder keeps only its marker, and what isn't
an event: the tools it was narrowed to, a turn asked to end, an answer
that finished a crashed turn.

### Why do Link Harness's conversations stay in files by default?

An agent runs in one place at a time, and a move carries its
conversations with its files folder, so on your own machines nothing
needs a store two machines share. A bucket syncs files, which made a
file per event look right; a store off the machine is a database driver
instead, which takes appends. Backups already put the files in your
buckets.

#### Why not a bucket of files

Keeping them in the bucket instead, for your Mac and a box to answer in
turn, would take each cloud's own API (rclone can't refuse a write
whose name is taken, so it can't keep one turn at a time), a bucket
tool outside the sandbox, and a copy here for memory.

## The copy, groups and forks

What the copy for people holds and why it only grows, how a group is a
conversation, and why a fork copies events.

### What does a conversation's copy hold?

What was said, up to the agent's last answer: the owner's messages and
the agent's answers, never a tool's call or its answer, and never the
owner's message while it's being answered. Memory searches the copy, so
a search would otherwise find the question it's searching for, and the
next search what the last one returned, an echo that grows with each
call. The events keep everything, calls and answers included.

### Why is the copy added to rather than made again?

The copy holds what was said up to the agent's last answer, so it only
grows, and only when the agent answers. Each answer adds what was said
since the last one to its end, from the messages the turn already
holds, rather than reading every message and writing the whole copy
again. When the copy doesn't end with the last answer (it was cut
short, or written elsewhere), it's made again from every message.

### How does Link Harness answer in a group?

Each group is a conversation of its own, `chats/<connection>-<group
id>`. Link's bridge takes your mention of the agent out of the message
before it goes on, and a mention with nothing else is ignored. Each
message is sent to Link Harness alone, and Link Harness adds the
conversation before it.

### Why do forks copy events rather than visible messages?

A visible transcript omits tool calls and results, provider thinking
and signatures, summaries and window marks. Rebuilding a conversation
from that transcript changes the model's prompt and loses its cacheable
prefix. A fork copies the complete immutable prefix before the edited
owner message, preserving timestamps and the source's tool restrictions.
Only the new suffix changes. This keeps each provider's normal cache
behavior without changing its cache keys or retention settings.

## The window

How much of a conversation the model sees, how a long one keeps going,
and where a model's window comes from.

### How much of a conversation does the model see?

A window: the conversation sent the same way each turn, so the
provider's cache holds, as much of it as the model takes. Past that,
the oldest turns leave half the window at a time, at points fixed by the
conversation itself, so the prompt's start stays the same for many
turns between drops. What leaves is summarized, and the window
starts with the summary.

### How does a long conversation keep going?

By compacting it. Once the conversation fills the share of its room
the specification says (`[window] compact_at`, three quarters in Link's), the
window moves on at its next drop point, as it always does, and what
leaves is summarized.

#### The answer first

The turn answers first, with the window it last sent: the room
`compact_at` leaves holds what came since, so the provider's cache
holds and the answer comes as soon as any. The answer goes to the chat,
and then, still under the turn's lease, the model is asked for the
summary as a continuation of that same prompt: the window, its answer,
and the request after them, which quotes the start of the first message
that stays. So the provider's cache holds for all of it, and the
summary costs little more than what it writes.

#### The summary

The summary is an event of the conversation, written once and never
changed, and the window starts with it until the next drop point, when
the next summary covers it and what left after it. Nothing is lost that
it didn't choose to leave out, and the prompt's start changes only at a
drop point. The summary never reaches the chat or the copy for people,
which hold the messages it stands for.

#### A window past its room

A window that has outgrown even that room (a long round of calls) is
summarized first, from what leaves alone, at most the walk's budget of
it, newest first, so the request never outgrows the window; then it's
answered.

#### The words asking for it

The words asking for a summary are Link Harness's, not a key: they're
the mechanism's, as a tool's description is, and what to keep is the
model's.

#### Saving notes first

Where the agent can write notes (`memory__remember`), the model saves
what matters to them first (`[window] save_notes`), in the same
continuation: a summary is for this conversation, and notes are for
every conversation after. So it saves only from a conversation its
owner spoke in: notes are about its owner, and one of others' messages
alone (a teammate's, a schedule's) could otherwise write them.

#### While it saves

While it saves, it may call nothing else, and with nothing to save,
nothing at all; its tools are still offered, as in every round, so the
cache holds for them too. The calls it made are kept with the summary.

#### A note changes the prompt

A note it saves changes the instructions from there on: a turn that
summarized before answering composes them again first, and every turn
composes them at its start, so the prompt after a save misses the
cache from the notes on, as after any note.

#### When it compacts

When and whether it compacts are the specification's, since each summary is a
call that costs: `compact_at = "never"` lets the oldest turns leave
unsummarized. A summary that fails, or is cut short, leaves them
unsummarized too, and the next turn tries again.

#### A message while it summarizes

A message that comes while it summarizes, or until its turn lets its
lease go, waits for a turn of its own: the answer it would have joined
is given. One written by another process sharing the store joins as it
would any running turn, and is answered in a turn after. Where an
interactions store coordinates the work, a message steered in meanwhile
moves the work's control revision; a summary the model answered is kept
all the same, and the message answered next. Only work halted meanwhile
refuses the summary.

#### A crash after answering

The answer is kept and sent before the summary is asked, so a turn that
fails or crashes after answering loses only the summary: the window's
mark stays where it was until a summary is written, so the next turn
answers with the same window and summarizes after it. A note it saved
before the crash may be saved twice.

### Where does a model's window come from?

From what's known of the model, which is data, not code: Link's file
of models' facts, refreshed at every release, under yours
(`~/.lnk/safe/models/facts.toml`), which wins field by field, so a
model newer than your Link works without a release, as [`lnk model
show`](../../../../../../model/docs/agents/models/providers.md#what-link-knows-of-a-model) prints it.

#### In what order

Link Harness takes the window from your file first, then from what a
runtime on the machine says it runs the model with (it knows better
than any file), then Link's, and gives a model nobody described the
specification's small window (`[window] unknown`, 32,768 tokens) rather than a
guess that overflows.

#### Ollama's window

Ollama runs a model with a smaller window than it was trained for
unless told, so a trained window Ollama reports counts only up to that
small one, unless you say otherwise.

#### How much of it to send

How much of it to send is the specification's. Link's `[window] max` is held to
the model's window; one you or a team set is taken as set, whatever
Link knows of the model: the window you set always wins, and the only
limit is the provider refusing a prompt as too long, which the harness
reports as that, saying the window it sent and where to change it.

#### The refusal

No provider has a common code for the refusal, so the harness tells it
by the message: Anthropic's "prompt is too long", OpenAI's and
OpenRouter's "context length", a runtime's "context window".

#### The conversation's share

The conversation's share is the window less the answer's (`[model]
answer`, else `[window] answer`'s share, at most the model's longest),
the instructions and tools, and the margin (`[window] margin`).

#### Counting tokens

Tokens are counted roughly before a turn, by characters (four ASCII
characters to a token, one for any other, so Chinese or an emoji isn't
undercounted), since each provider's tokenizer is its own; the margin
covers the difference. After a turn, the provider's own counts are
what's kept, measured and priced. The window's drop points stay on the
rough count, which never changes for a message once written, so the
provider's cache holds.

### How does a turn find the window without reading every message?

Each answer records the window's checkpoint: the starting message, its
time and the tokens before it. A turn reads the latest checkpoint from
the last 32 messages and resumes counting from there, or from an earlier
answer or summary when needed. The checkpoint stays with the events that
determine it.

#### Reading the log

Log reads search backwards by sequence number and load only the
required lines. Each line starts with its sequence number, so the search
reads a line's head, not the whole line. A summary counts as one message,
while the complete conversation remains available for reconstruction.

#### Reading only what was added

A turn reads its conversation more than once, and every turn reads it
again. So the store keeps what it last read of each log, with where its
last line ends, and the next read parses only the lines added since.

#### What the store keeps of a log

The store keeps only the events from the first a turn reads by the
latest window mark: where the window starts, or the last answer or
summary when either is earlier. It also keeps the last 32 messages,
which a turn reads to find that mark. Older events are let go once a
mark passes them. A process's memory then follows its windows, not the
length of its conversations.

#### Reading a log again whole

The store first checks that the file is the same one, at least as long,
with the same bytes where its first and last read lines were. A log
moved, or deleted and begun again, is read again. What it keeps is
derived: a new process reads each log from its window's start once, and
keeps up to 64 logs, forgetting the one read longest ago. A model
request's record is shared, not copied, as a turn passes its events on.

#### Compact and driver conversations read on

A conversation kept compact or by a store driver has no lines to check,
so the store keeps what it last read of it, its window's events, and
the next read asks for the events from the last one it holds. When that
event comes back as it was, the rest is what was added. When it doesn't
(the conversation moved, or began again), the store reads from the
window's start again.

#### A model request records what changed

Each model request records the messages it sent, as digests, so what
the model saw can be checked later. Listing them whole every time would
grow the log with the square of the window: at a window of a thousand
messages, every request would add about 650 KB. So a request records
what changed since the request before it: the earlier request's
messages it keeps, from which and how many, and the messages it adds. A
turn's record then grows with what the turn added.

#### When a request is recorded whole

The process keeps each conversation's last request to compare with. A
request is recorded whole after a restart, when the window starts anew
with a summary, and every 256 requests, so making one whole never walks
back further. The evidence index makes every request whole again from
the ones before it.

#### Each Message Digested Once

A digest here is a message's SHA-256, and a request names every message,
call, answer and image it sends by one, and where each event's words
went. Made again at every request, they would cost what the window
holds, every request.

#### What the Process Keeps of Them

The process keeps each conversation's digests, never the bytes they're
of. A request finds a message the one before sent by its length and a
128-bit SipHash of it, keyed by the process alone. Before each request
it forgets what the request before didn't send. So it keeps about a
window's digests for each of the 16 conversations asked most recently,
however large their images. A request then makes a SHA-256 only of what
it adds. The keyed hash costs about what SHA-256 does on a processor
with SHA instructions, and a fraction on one without.

#### Finding a message sent again

A source may send a message again, such as a retried delivery or a task
given twice. The harness then answers with the message it already
holds. It finds that message by its delivery or its sender. The store
keeps, for each log, where the first line naming each one starts. That
record grows by the lines added since it was last asked, parsing only
lines that name a delivery or a sender. A new message never reads the
whole conversation.

### How does Link Harness find a chat's goal without reading every message?

After each of your answers the harness looks for the chat's goal, which
the conversation's messages hold: the `/chat:goal` that set it, the
turns and tokens spent on it, and the command that ended it. Reading the
whole chat each time would make every answer cost the chat's length. A
conversation kept compact or by a store driver, read whole, also takes
far more memory than its window while it's read, and the process keeps
that memory.

#### Reading On From the Goal Found

So the harness keeps the goal it found, as of the chat's last message,
and reads on from there through the window. A chat whose last message
isn't there as it was, one where a conversation began since
(`/chat:create`, a fork, or a resume), and one a new process meets are
read whole once.

## The prompt cache

A provider keeps a prompt it was sent for a while, its cache, so the
next request that starts the same way reads that part for a fraction of
the price. How long Link asks it to keep one, and when a quiet
conversation is compacted, are chosen by what they cost.

### Why choose how long the cache keeps a prompt?

Because the cheapest lifetime depends on the conversation. Writing to
Anthropic's cache costs 1.25 times the input price for five minutes, or
2 times for an hour, and reading from it about a tenth, a twentieth on
Claude Opus 5.5. A message that comes after the cache ended writes the
whole prompt again. So each lifetime costs, for a request and the next:

```text
write × written + prompt × (read × F(lasts) + write × (1 − F(lasts)))
```

#### Reading the formula

Every figure is a multiple of the input price. `written` is what the
request adds to the cache, `prompt` what the next request reads, and
`F(lasts)` the share of the conversation's pauses shorter than the
lifetime. The cheapest is asked for. With a 40,000-token conversation
that adds 1,500 tokens a turn, an hour wins once more than about one
pause in fifty falls between five minutes and an hour. Losing the bet
costs the dearer write of 1,500 tokens, while winning it saves writing
40,000 again.

#### Learned from the conversation

The pauses are the conversation's own, its latest twenty, after what's
assumed of a new one (`[connections] pauses`). A chat with quick
replies and a pause or two an hour asks for an hour. Rapid fire, then a
night's silence, asks for five minutes, since the hour's dearer writes
would buy nothing.

#### Anthropic and OpenAI

Anthropic offers five minutes and an hour, so both are costed. OpenAI's
GPT-6 models keep a prompt at least thirty minutes at 1.25 times, unasked,
on the Chat Completions API Link sends them, so there's nothing to choose.
Its twelve hours are offered only on its Responses API. Each provider's
offers are its models' facts, so a new one is data, not code.

### Why does a quiet conversation compact?

A chat on Telegram is one conversation that goes on for days. Without
compaction, a message hours later writes the whole conversation to the
cache again. Compacted while the cache still holds it, everything
before the owner's last message is one summary, asked as the last
answer's prompt up to that message, so its input is read cheaply, and
the next message starts small.

#### When it pays

A summary costs reading the prompt, writing the summary at the output
price, and caching it. It saves the history it replaces, written again
by a message after the cache ended, and read by each turn after. With
`cold` the share of pauses that outlast the cache and `turns` the turns
a conversation takes after a pause, three:

```text
compact when (history − summary) × (cold × (write + (turns − 1) × read)
                                   + (1 − cold) × turns × read)
             > prompt × read + summary × (output + write)
```

#### What that comes to

On Claude Opus 5.5 with a 2,000-token summary, an hour's cache and a
night's silence, it pays from about ten exchanges, whether they took
nine minutes or three hours. Below that, reading the conversation again
is cheaper. A conversation that comes back within the hour finds its
cache warm, so a summary saves only cheap reads, and it waits for a far
longer conversation.

#### When it's timed

It's timed five minutes before the cache the last turn asked for ends:
at 55 minutes with Anthropic's hour, at 25 with OpenAI's thirty. A cache
of five minutes leaves no time inside it, so it isn't compacted for
being quiet. `compact_after` may name a time instead, always kept.

#### Which conversations

Compaction by tokens holds everywhere, since it keeps a conversation
from outgrowing its window. Compaction by time is a bet that the
conversation goes on. A conversation of [`lnk agent chat`](../../../../../../agent/docs/agents/chat/README.md#manage-history)
mostly ends when you leave it, so Link's specification sets `never` for the
`link` connection, and a conversation on no connection never compacts
by time.

#### Silent

The summary is the agent's own, so nobody is told. The log says it
compacted, and each summary reaches the model with when it was written,
so the agent can say when it compacted if asked.

#### Woken by the scheduler

An idle agent holds nothing after `[run] idle_after`, so a timer of the
harness's own would be gone by then. The agent's scheduler keeps it, as
a job of the conversation's whose ID comes from the conversation's
name. Each turn's end moves the one job, or takes it away when a
summary wouldn't pay. The harness knows its firing by that ID. An agent
without the scheduler compacts by tokens only.

#### Through `cron`

The harness keeps the job with the scheduler's `compact`, which it
reaches through the `cron` namespace it's handed, on the owner's machine
and on a box alike. [The namespace](../../../../../portability/contract/docs/agents/harnesses/more/protocol/settings.md#namespaces) offers that call to no model.

#### A firing with nothing to do

A firing that finds a turn running does nothing, since that turn moves
the job again. One that finds a summary written since the owner's last
message does nothing too: that turn compacted by tokens, and its
summary was never a prompt's start. A firing missed while the computer
slept is dropped by the scheduler, since its cache is gone.

### Where do the choice's figures come from?

From three homes, each by what it is. Each model's cache offers and
prices are its facts. The figures anyone would choose (the five
minutes' margin, three turns after a pause, a first summary's 2,000
tokens, twenty pauses learned from) are the model contract's
`caching.toml`. What's an opinion, a lifetime or a time to compact
always and the pauses assumed of a new conversation, is the Agent Specification's
`[connections]`. The arithmetic itself is one module,
`link_model_contract::caching`, which reads and sends nothing.

### How sure are the defaults?

The defaults are an estimate: the pauses assumed of a new chat, the
three turns after a pause, and the margin. A summary's real length is
learned once one is written. Link Harness doesn't use OpenAI's
twelve-hour cache, which only OpenAI's Responses API offers.

## Tools

How Link Harness reaches its tools, keeps their calls, and holds their
sessions between turns.

### How does Link Harness get tools?

As an MCP client of the servers Link's tool bridge already runs outside
the sandbox (`$LNK_TOOL_<NAME>`, memory at `$LNK_MEMORY`, and `run`,
`file`, and `code` at `$LNK_RUN`, `$LNK_FILE`, and `$LNK_CODE`), never
as code of its own. The scheduler reaches it only through the
namespaces over it, `task`, `cron`, and `agent`. A tool written
once works for every harness, the calls you allow and the ones that ask
are the same for each, and Link Harness still runs no code.

#### Their names

Memory is the first. Each tool is offered to the model as
`<tool>__<call>`, so two tools can have calls of the same name, and
memory's are `memory__search`, `memory__read`, `memory__list` and
`memory__links`. `task__run` runs a shell command, and `file__read`,
`file__write`, `file__edit`, `file__glob` and `file__grep` work on files, each
call in a sandbox of its own. The tools are listed the same way each
turn, so the provider's cache holds.

#### A command in the background

`task__run` with `run_in_background` runs a command in the background,
which outlives the turn and the tool's session that started it, and
`task__list`, `task__read` and `task__stop` reach it. Link Harness keeps nothing of it: the command's end comes
back as a message from `run:<id>`, and Link Harness says who's
speaking, as for a schedule.

#### A team's tools

A team's tools come the same way: its memory over the team's folder is
`$LNK_TOOL_<TEAM>_MEMORY`, offered as `<team>_memory__search` and the
rest, one server for every member.

#### A subagent's tools

A subagent's task can come with a shorter list of tools (`Link-Tools`),
which Link Harness keeps in `tools.json` beside the conversation's
events, or with the conversation in its store when its machine keeps
conversations off the machine (`tools_narrow`, narrowed there in one
step): never on a disk that a sleep empties, where losing it would give
a woken subagent every tool. A later list narrows it again and never
widens it, and a call to a tool the subagent wasn't offered is answered
as one there isn't.

### How are a tool's calls kept?

As events of the conversation, like any message. The model's answer
that makes calls holds them, and each tool's answer is an event of its
own after it, written as it happens. A turn cut short leaves the
conversation as far as it got. A call left without an answer is
answered in the next prompt as unanswered, so the prompt stays valid.
With the tools gone, the calls and answers go to the model as text.

### Does a turn hold anything of its tools?

Only the sessions of memory, `run`, `file` and `code`, and of the
`task` and `file` namespaces over them, kept between turns: `code`'s
holds its language servers, which index a project once.
A turn opens a session with a tool only when the model calls it, so a
message that calls no tool starts none. Every other tool's session ends
with its turn.

#### Memory's session

Memory's is kept for the turns after, and ended once none has used it
for 10 minutes, or an hour after it started, so what it started without
(its model, not up yet) is tried again: ending it with each turn
started its process again with every message that called it (its
sandbox, its model server and its index), and its embedding ran only
inside turns.

#### `run`'s and `file`'s

`run`'s and `file`'s are kept as long, so a turn that runs a command or
reads a file doesn't start a server and list its tools first. So are
the `task` and `file` namespaces, which reach them, and the scheduler,
through Link's namespace server. Retaining those sessions avoids starting
and listing the same providers on every warm turn. Their configured idle
and maximum-age limits bound their lifetime.

#### One process for every conversation

A kept session is one process for all of an agent's conversations at
once, and a tool can keep what a conversation did in it, such as a
browser's pages or a shell's folder: memory, `run`, `file`, the
scheduler and the namespace server are the agent's own in every
conversation and keep nothing of one in their session (each call says
its conversation, and the namespace server passes that on), but
another tool's state from one person's turn mustn't reach another's.
The `tool` and `agent` namespaces still end with each turn.

#### Derived from the tools

Each tool's list of calls is kept in the process between turns, and
asked again whenever a session starts. Both are derived from the tools:
a process that hasn't listed them yet lists all of them at once when
its first turn starts, and starts its own sessions. The conversation's
events hold everything else the next turn needs, so any process on any
machine can take it.

#### Starting a tool

A turn waits 30 seconds for a tool to start and list its calls. A
start still going then isn't thrown away. It goes on apart from the
turn, for at most three minutes in all, and the turns after wait for
that same start rather than begin another. On a busy machine, where
several agents start at once, a slow start is finished once and serves
the next turn. Past three minutes it's given up on and its connection
closed, so the tool bridge can end it, and a session it started is
ended too. A tool that fails to start after it was listed is still
offered, and a call to it is answered as unavailable.

#### Waiting for the grant

A process's first ask for a tool waits for its sandbox to grant it.
Right after a restart, the sandbox can refuse a tool (HTTP 403) before
it answers for the agent's grant. Link asks again every half second,
within the start's time, rather than warn and offer an old list.
A later refusal in the same process is said at once, since that grant
was there before.

#### Their order

A server's tools are offered in order of name, whatever order it lists
them in. So the tools, and the prompt they start, stay the same, and
the provider's cache holds.

#### `tool-lists.json`

The lists are kept beside its settings too (`tool-lists.json` in its
home), so a new process does the same with a tool that fails to list
its calls when it starts, rather than leaving it out. A restart with
one tool down doesn't change the prompt, then change it back.

#### Asked again each turn

A saved list stands in only until the tool answers. Each turn asks the
tool again, and what it lists then replaces the saved list, in the
process and in the file. So a call added to a tool since the file was
written appears at the first turn the tool answers.

#### What the file holds

The file is derived: it's read once a process, written only when a list
changes, holds only the agent's tools now (a tool taken away, or moved,
isn't offered from it), and a process without it lists the tools as
before. It holds each tool's name, a hash of its address (which carries
the proxy's secret), and its calls, and each agent a process serves has
its own. A tool that was never listed and failed to start is left out until
its retry window opens. Link retries after five minutes. One still
starting isn't a failure, and is offered once its start is done.

### How many calls does a turn make?

The specification’s `[turns] rounds` bounds tool-call rounds, eight in Link’s
defaults. The model is then asked once more and told to call nothing.
`[tools] answer`, or the tool’s own `answer`, bounds retained tool text,
32 KiB by default. A message that steers a running turn is answered
even past the last tool round.

### In What Order Do One Round's Calls Run?

A round is one model answer, and it may ask for several calls. Link
Harness runs them in the order asked, except that calls side by side
that only read run at once. A call that changes something starts only
after every call before it has ended, and the calls after it start only
once it has ended. A model that deletes two schedules and then lists
them in one round sees the list after the deletes.

#### How a Tool Says a Call Only Reads

A tool says a call only reads with MCP's `readOnlyHint` annotation, as
it lists its tools. Link's own tools declare it for each call in their
`actions.toml` (`read-only` in the call's `[tool]` table). A call whose
tool doesn't say it only reads is taken as a change, so a tool that
says nothing runs one call at a time. A tool offered as a process kept
its list, because it failed to list this turn, is taken the same way,
since only a listing says what reads. Running every call at once was
faster, but a list beside a change answered from before it, and the
model believed what it saw.

#### Answers Keep the Asked Order

Each answer goes back to the model in the order the model asked,
whatever order the calls ended in. The conversation's log keeps each
answer as it ended.
