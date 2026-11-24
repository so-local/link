# Context Reporting

See what your agent's context is spent on, part by part, for every
harness: its instructions, each tool, and the conversation down to each
tool's calls and results.

## See a chat's context

`lnk agent model context` answers with an agent's latest chat, part by
part, and with the chats it keeps calls of. A request naming a chat
answers with that one, and `/context` in a chat answers with its own.

```sh
lnk agent model context --agent main
lnk agent model context --agent main --request chat.json
```

### Name a chat

A request is a JSON object with an optional `chat`, as the list of
chats in an answer names it:

<!-- json: link_model_contract::api::ContextOf -->
```json
{"chat": "main"}
```

### Read the report

The answer holds the chats kept, the latest first, and the chat's
report, shown as fields and tables, and as JSON with `--json`. That holds its latest call, each part's path, bytes, tokens and
cost, and each part summed over the chat's kept calls. A part's path
says where it is (`instructions/skills/tides`), so a total for
everything beneath a path adds up its parts.

## What the parts are

A part is named by a path that stays the same from call to call, so one
call compares with the next. The census reads the request each call
sends its provider, whichever harness wrote it, and keeps these paths.

- **`instructions`:** the system prompt, or OpenAI's system and
  developer messages and Responses' `instructions`.
- **`tools/<name>`:** each tool's definition and schema, as offered.
- **`conversation/user` and `conversation/assistant`:** what each side
  said, as text.
- **`conversation/thinking`:** the model's thinking sent back with its
  calls.
- **`conversation/calls/<tool>`:** each call's arguments, by tool.
- **`conversation/results/<tool>`:** each tool's results, named by the
  call they answer, an image a tool returned among them.
- **`conversation/media/<kind>`:** images, documents and audio a person
  sent.

A tool whose name isn't a plain name, or one past the census's 256
parts, is counted in its kind's `other`.

### Each section and skill by name

Link Harness composes its system prompt, so it sends with each call
where each section is (`base`, `model`, `tools`, `environment`,
`repository`, `skills`, `persona`, `owner`, `standing`, `notes`) and
each tool's instructions and skill in it. Each shows beneath
`instructions`: `instructions/persona`, `instructions/skills/tides`,
`instructions/tools/memory`. An outside harness's system prompt is one
part, `instructions`.

## How tokens are known

Each part's size in the request is exact. Its tokens are the call's
measured prompt, as the provider counted it, split among the parts by
how many tokens each kind takes: text by its characters, an image as
about 1,600, a PDF by its size. The parts add up to the measured prompt
exactly, and the report says `apportioned`. Where the provider's answer
gave no usage, each part's own estimate stands, and the report says
`estimated`.

### What it adds up over the chat

The over-the-chat column adds each part's tokens across the chat's kept
calls. Instructions and tools are sent again with every call, so they
show here at what they cost the chat, not what they take of one window.

### What it cost

Each part's cost is its share of the prompt's cost, by its tokens: the
prompt at the model's price when it was asked, its tokens read from
the provider's cache at the cached price, from [what Link knows of the
model](more/details.md). The answer's cost is apart. A
cost whose price isn't known is reported as unknown, never as nothing,
and so is a sum over the chat holding one.

## Which chat a call is in

Link Harness names each call's chat by its thread. An outside harness
names no chat, so its calls are grouped by the opening message they
share, `opening-` and its fingerprint. Compaction that rewrites a
conversation's start begins a new one.

### The work a chat asked for

Link Harness keeps a thread's compaction and goal calls apart, under
the thread's name with `/compaction` or `/goal`, each naming the thread
as its parent. The thread's latest call is always its own, and its
over-the-chat view counts that work too, as `compaction/…` and
`goal/…`. A report without a chat named is of the latest chat a person
had.

## Where it's kept

The model bridge keeps each call in Link's settings, in
`~/.lnk/data/models/context/<agent ID>/`, a file for each chat, readable by
you alone. Each line is one call: its chat, its place in it, its time,
its model, the provider's counts and its parts' paths and sizes. Never a
word of the prompt or the answer is kept. A chat's file keeps at most
4 MB, its oldest half dropped past it, and an agent's at most 64 chats,
the one least recently used dropped for a new one. An agent's records
go when it's removed, with [`lnk agent delete`](../../../../agent/docs/agents/start/delete.md#delete-your-agent).

## Limits

A call is kept from every way a model is asked: an outside harness's
own request to its provider or to a runtime on this machine, and Link
Harness's through the model actions. A request to a local runtime over
32 MB streams through unread, and isn't kept. [The report](more/api.md) reads only the
calling agent's own calls.

### Forks and moves

A forked conversation is a thread of its own, and so a chat of its own,
its calls from the fork on. The records are kept by the bridge of the
machine the agent runs on: an agent moved to a box keeps new records
there, and those left here go when it's removed.
