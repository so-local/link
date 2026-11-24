# Conversations

Link Harness retains conversations in your agent’s files or through its
storage driver. Read their history, choose where they are kept, and
control how much the model sees on each turn.

## Read your conversations

Each chat's folder is named by its connection and chat, and
`conversation.md` in it is [a copy to read](../../../../../tools/memory-tool/docs/agents/tools/memory/guide.md#what-it-finds), added to as the agent
answers.

### `events.jsonl`

`events.jsonl` holds each message as a line of JSON, in order, only
ever added to: a line is never changed once written. It holds the
message's time, model, tokens as the provider counted them, and cost.

### Event Schemas

Each line names its saved-file schema. The same schema applies to
compact frames and store-driver records, and remains attached when a
conversation moves between them. A newer schema is refused with an
upgrade message. A line without a schema follows the shared saved-file
policy and is read as the current shape.

### Derived Conversation Copy

Once an event is appended, a failure updating its Markdown copy is
reported separately and does not retry the committed event. A later
answer rebuilds the copy from the retained events. Internal execution
records stay in the event store and are excluded from this readable copy
and the memory search projection.

### What its events hold

A tool's call is in the message that makes it, and its answer is a line
of its own, as is a summary of what left the window, and a command you
ran, such as [`/chat:create`](../../../../../agent/docs/agents/chat/README.md#manage-history).

### `lnk agent chat show`

[`lnk agent chat show <chat>`](../../../../../agent/docs/agents/chat/README.md#talk-to-it-here) prints a chat as `conversation.md`
holds it, however it's kept.

```sh
lnk agent chat show telegram:123456789
```

## Open and fork a Link conversation

History shows only the owner's messages and the agent's answers, with
their original event numbers and UTC times.

```sh
lnk agent chat list --json
lnk agent chat show link:main --json
lnk agent chat fork link:main --before 7
```

### The fork

The fork copies every event before the selected owner message into a
fresh `link:fork-…` thread and prints its history. Its source remains
unchanged. Send the edited message to the new thread with `lnk agent
chat send --chat <name>`.

### What it copies

The copied prefix includes tool calls, results, provider thinking and
signatures, summaries and window marks, and keeps the source's tool
restrictions.

### The provider's cache

OpenAI, Anthropic, OpenRouter, Bedrock through a model service, and
other providers receive their usual cacheable prompt prefix; only the
edited suffix changes. Provider cache lifetimes and minimum sizes still
apply.

## Separate chats

[`lnk agent chat`](../../../../../agent/docs/agents/chat/README.md#manage-history) opens a new chat by default, with its own folder
under `agent/chats/`. Follow-ups stay in that session. Select a named
chat with `--chat <name>` to continue it. Discord and Slack threads have
distinct chats too; replies in one thread share its context.

### Saved memory across chats

Saved memory notes are shared by the agent. A fact only mentioned in a
chat is transcript history until the agent saves it with its memory
tool.

## Keep them smaller, or in a database

How a new chat is kept is your agent's to say, in its specification's [`[storage]
conversations`](../../../../../agent/contract/spec/docs/config/specs/guides/agent.md#storage).

```toml
schema = 1
[storage]
conversations = "compact"
```

### `log`

Link's. Each chat's folder holds `events.jsonl` and `conversation.md`,
read with `cat`, `jq` or `lnk agent chat show`. A specification that names
`file`, the agent's own folder, as [`lnk agent spec show`](../../../../../agent/docs/config/specs/agent.md#use-it) prints it, keeps
each chat this way too.

### `compact`

Each chat's folder holds `events.zst`, its messages in zstd frames, and
`events.jsonl`, those since the last frame, read with `lnk agent
chat show`. `compact` compresses the event log to reduce disk use.
It reads the whole conversation for each turn, including requests for
its latest messages, so a long compact conversation needs more read and
decompression work than a log or store driver.

### `sqlite`

[A store driver](../../../../../stores/contract/docs/agents/more/stores/protocol.md#store-drivers-lnk-name) is a module, `lnk-<name>`,
named here by its name: each chat's folder holds an empty `store.sqlite`,
and the driver keeps its messages. Link's is SQLite: `lnk mod add
sqlite-store`, then `conversations = "sqlite"`, and every chat of the agent is
in one file, `agent/chats/events.sqlite`.

#### Where the driver runs

Its program runs outside Link Harness's sandbox, behind the agent's
store bridge, which takes each call only as the agent's own; memory
runs it in memory's sandbox.

### Move the old ones

Changing the key moves nothing: each chat stays where it is, read
there, and a new chat starts the new way. To move the old ones too,
stop your agent and rewrite them the new way, here `compact`.

```sh
lnk agent stop
lnk agent chat export --format compact
```

### What isn't a message

Whatever the way, a chat's folder holds what isn't a message as files
of its own: the tools it was narrowed to (`tools.json`), and while a
turn runs, its lease (`turn.json`; a driver keeps that too) and whether
it was asked to stop. Memory searches a compact or driver's chat as it
does a log's: it makes the copy itself.

### Through the storage bridge

Handed the storage bridge (its settings' `storage`, which `lnk agent
start` gives it on every machine), it reads and writes whatever its
machine keeps off its folder (in SQLite, a bucket, or a box's store)
only through it. What's kept at `file` stays in its folder as above.

#### Nothing on the disk

The driver of that place keeps a chat's messages, its lease and the
tools it was narrowed to, and whether a turn was asked to stop is held
in memory, so nothing of such a chat is on the machine's disk. A turn
whose tools it can't read fails, rather than offer every tool.

## Messages from its schedules and subagents

Its [scheduler](../../../../../tools/scheduler-tool/docs/agents/tools/scheduler/README.md)
lets it start subagents and schedule messages to itself, on a
[team](../../../../../agent/docs/agents/team/README.md#make-a-team)
message its teammates (`agent__send`), and a background command it
started tells it how it ended.

### As from them

Such a message comes as from them, never from you. It's kept with who
sent it, the model is told, and each line of it is quoted, so nothing
in it reads as yours: in `conversation.md` too, where it's headed
"Others", not "You". Its prompt names each teammate with its agent's ID
and purpose, and what of the team's folder it may reach. Each folder
is named whole, as the file tool takes it (`teams/research/drafts/`),
and the folder's place on disk only as where you find it.

## Send another message while it answers

A message you send while it answers is kept at once, and then does what
its specification says in `[turns] busy`.

### `[turns] busy`

`steer` is Link's: the answer takes the message in at its next step,
and goes on to answer it too, the reply coming on your first message.
`queue` answers it after, on its own. `interrupt` stops the answer
where it is, keeps it marked interrupted, cancels the tools still
running, and answers your new message.

### After a crash

After a crash or restart, the harness attempts to resume turns whose
leases have lapsed. A recovered answer is kept for the bridge to send
to the chat. If the bridge stops while the harness remains available,
the harness can finish the turn and retain its answer for delivery
after the bridge returns. Delivery still depends on the model, tools,
storage, and connection recovering.

## See what it costs

After each turn the harness reports numbers only: its tokens, its cost,
its tool calls and failures, and how long it took to ask the model, to
send the first piece of the answer and to finish it.

```sh
lnk measure --agent main
```

### What a turn counts

Each turn counts only the answers and tool calls it wrote itself, even
when the next turn has begun by the time they're counted. They never
include the words. [The `measure` module](../../../../../../tooling/measure/docs/tooling/measure/README.md) collects them.

### A message's cost

A message's cost is the provider's when it reports one, and otherwise
its tokens at the model's price, which [`lnk model
show`](../../../../../agent/docs/agents/models/agent/README.md) prints.

## How much it remembers

The model sees as much of the conversation as the
[specification](../../../../../agent/docs/config/specs/agent.md) sends,
up to 128,000 tokens in Link's defaults, capped by the known model
window. That bounds context per request rather than a conversation’s
total cost.

### Its window

What Link knows of a model sets its window, as [`lnk model
show`](../../../../../agent/docs/agents/models/agent/README.md) prints it,
and your own figures win. A model on your computer gets [the window its
runtime says it runs with](../../../../../model/docs/agents/models/more/details.md),
and a model nobody described gets 32,768
tokens. A `[window] max` you set is used as you set it, whatever Link
knows.

### Past three quarters

Past three quarters of it, the oldest turns leave the window half of it
at a time, so the provider's cache keeps the prompt cheap. What leaves
is summarized, once the turn has answered, and the window starts with
the summary, so a conversation keeps going however long it gets. The
files keep everything.

### After a quiet hour

A chat on Telegram, Slack, Discord or Signal is one conversation that
never ends, so it also compacts by time. Once it has been quiet until
just before the provider's prompt cache ends, everything before your
last message leaves for one summary, while its input is still read
cheaply. Your last message and its answer stay. Your next message,
hours later, then starts from a small window rather than rereading the
whole chat at full price.

#### Only when it pays

A summary costs something too, so a short chat isn't compacted: reading
it again is cheaper. The cache's lifetime is chosen the same way, an
hour from Anthropic when your replies come minutes apart, five minutes
when they come in quick bursts and then stop. With Anthropic's hour it
compacts at 55 minutes of quiet, with OpenAI's thirty minutes at 25.
The arithmetic is in [Decisions](more/decisions.md#the-prompt-cache).

#### Only where a conversation goes on

A conversation of `lnk agent chat` is mostly done once you leave it, so
it compacts by tokens only, and so does a conversation on no connection,
such as a subagent's. Compacting those would spend a summary nobody
reads.

#### Silent, but known

Nothing is sent to you when it compacts. The agent's log says so, and
the agent sees when its summary was written, so it can tell you if you
ask.

#### Woken by the scheduler

The agent's scheduler wakes it for this, so it happens only where
`scheduler` is among the agent's tools (`[tool.scheduler] enabled =
true`). Each turn sets the time again. A conversation already compacted
since your last message isn't compacted twice.

### Notes and the summary

Where the agent has notes (Link Memory's `remember`, with `lnk agent
memory start`), the model saves what matters to them before it
summarizes. The summary is kept in the conversation's events, and never
sent to you.

### `compact_at`, `compact_after` and `cache`

When it compacts, and whether it saves first, are `[window] compact_at`
(by tokens) and `save_notes`, and `[connections] compact_after` (by
time). How long the cache keeps a prompt is `[connections] cache`, and
what's assumed of a new chat's pauses is `pauses`. Link's specification sets
them so.

```toml
schema = 1
[connections]
cache = "auto"
compact_after = "auto"
pauses = ["1m", "1m", "20m", "8h"]

[connection.link]
compact_after = "never"
pauses = ["1m", "1m", "1m", "1d"]
```

#### Your own choices

A connection's own table wins over `[connections]`. `compact_after` takes
a time of quiet to compact after always (`"55m"`), and `cache` a
lifetime the provider offers (`"1h"`). `compact_at = "never"` lets the
oldest turns leave unsummarized, and turns the quiet compaction off too.
