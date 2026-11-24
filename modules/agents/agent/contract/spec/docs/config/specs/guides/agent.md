# Agent Specification Guide

How the specifications an agent runs meet, where each is kept and how its values are written. Each field is in the [Agent Specification](../../../../../../docs/config/specs/agent.md) reference.

## Opinions, not decisions

A decision is in the code: what anyone with the facts would choose,
such as Telegram's 4,096 characters a message, or how often its
documentation says a bot may write. A model's own figures are facts too,
so they are data. Its window, longest answer and price are in Link's file
and yours, and [`lnk model show`](../../../../../../../model/docs/agents/models/guide.md#where-things-live) prints them.

### An opinion

An opinion is a key here: a trade reasonable people make differently,
such as how much of the window to pay for. Link's own opinions are a
specification of their own, `link_core`'s `agent_spec/link.toml`, printed whole
by [`lnk agent spec show`](../../../../../../docs/config/specs/agent.md#use-it), so someone with the opposite ones changes only the file.

## Specifications meet as sets

An agent runs three specifications as one: Link's, then each of its teams', then
its own. Each is a set of keys, and any of them may be partial. The agent
runs their union, with a key from a later set winning, key by key. A
table of yours replaces only the keys it has.

### What `lnk agent spec show` prints

The union, each key marked yours, a team's or Link's, and `[storage]` as
this machine keeps each collection, a place it overrode marked this
machine's, naming the one the agent prefers (in `--json`'s `from`:
`{"from": "machine", "name": "<preferred>"}`).

### A general key beats the specific ones before it

Your `[model] default` replaces Link's `[model.anthropic] default` too,
your `[connections] working` Link's `[connection.slack] working`, and `[tools]
answer` a `[tool.<name>] answer` set before. A specific key in the same
set as the general one still wins for its provider, connection or tool.
An empty reaction (`working = ""`) is none.

### The union must be whole

Every key Link's specification has must be in it, but for those with a fallback
(`[window] max`, the model names and the reactions) and each tool's own
(`[tool.<name>]`). With `extends = "none"`, Link's set is empty, so
yours must have them all. A key no set holds refuses the start and
names it. A feature's switch is never needed: left out, the feature is
off.

### A tool's keys

A tool's keys are its tool's to need: a tool the agent runs says which
it lacks when it starts (`its spec sets no [tool.scheduler] per_day`),
so a specification that extends none sets only the tables of the tools it runs.

### Teams agree, or you settle it

Two teams setting one key differently refuse the start, unless the
agent's own specification sets it.

### What isn't an opinion isn't yours to claim

`name` is what Link knows of the agent here: a specification naming another
agent is refused. Whose it is and its teams are no specification's keys at all:
its saved setup keeps its owner and each team's file its members, so
`lnk agent spec show` prints neither.

### Where is the machine's to grant

`[storage]` is a request: each collection is [kept where the union
says](../../../../../../../../../docs/config/README.md#where-an-agents-things-are-kept) if the machine supports it, else at the machine's default, and
`lnk agent spec show` marks each key the machine overrode.

### Counts are yours

`rounds`, `calls` and the scheduler's caps take any number from 1 to
65,535. Within that range, counts above the warning thresholds of 64
rounds, 128 calls, or 64 subagents or hops are allowed, and
[`lnk agent start`](../../../../../../docs/agents/README.md#create-an-agent) says so.
The scheduler’s daily-message cap still cannot exceed 65,535.

## Storage

Everything an agent keeps is in six collections, each read and written
through one storage bridge, whatever keeps it. On `file`, `home` is
`Home/` and each other `Agent/<collection>/`.

### `instructions` and `skills`

Its instructions and its skills, which you edit. `[instructions]`'s
files are in the agent's collections, their first part naming which:
`instructions/base.md` is in `instructions`, and the agent's notes,
`memory/notes.md`, in `memory`.

### `home`, `memory`, `conversations` and `schedule`

`home` is the person's: documents, work, and the documents memory
searches. The others are memory's notes and index, its conversations,
and its scheduled jobs.

### How a place keeps one

A place keeps a collection however its driver does, the same to the
agent and its tools: the bridge reads, writes, lists and removes, adds
to a file at its end, and leases a turn, so two copies of an agent
never both write one. A harness from outside Link keeps its own
conversations however it does; the rest are Link's.

### `file`

Each collection [a folder of the agent's](../../../../../../docs/agents/more/settings/README.md#an-agents-folder) on this machine;
a conversation is a file only added to. It keeps every collection.

### `sqlite`

Each collection rows of one SQLite file in the agent's folder,
`lnk-sqlite-store`. It keeps all but `home`.

### `bucket`

Each collection objects in the agent's bucket, the one [`lnk storage`](../../../../../../../../storage/storage/docs/services/storage/README.md#storage)
connected for it, encrypted, with a cache of memory's index on this
machine. It keeps every collection.

### A place's driver

A place whose driver isn't installed installs as a module a feature
needs does. A place's driver refuses what it doesn't serve, and the
machine's `[storage]` never lists a place for a collection its driver
refuses.

## Values

How a specification writes a size, a duration and how hard a model thinks, and
what it refuses.

### Sizes and durations

A size is a number and `B`, `KB`, `MB` or `GB`, powers of 1024. A
duration is a number and `ms`, `s`, `m`, `h` or `d`.

### Thinking

On Anthropic's newer models, the effort the model thinks at, and `off`
the least it can, as some always think. On a model that takes a budget,
`thinking_budget` tokens, under the answer's. On OpenAI's reasoning
models, the reasoning effort. A model Link doesn't know isn't asked to
think.

### A tool's own keys

`tool.memory`'s keys are memory's only, `tool.scheduler`'s the
scheduler's, `fetch` web's, and `output` run's. Another tool's table
with one is refused. Link Harness sends each tool its own table with
every call, so one tool shared by many agents follows each one's.

### What's refused

An unknown key or table, an empty table, or a value out of range is
refused, naming the line.

## Where it's kept

Yours is the agent's, a team's is the team's, and neither grants what
the agent reaches.

### Yours

`lnk agent start --spec <file>` reads a standalone behavior specification strictly,
checks its resolved values, and keeps them under `[behavior]` in
`~/Link/Safe/Agents/<agent>/agent.toml`, beside its setup. Generated
configuration contains concrete defaults without an `extends` pointer.
Examples on this page are standalone specifications. When editing saved
configuration, prefix their sections with `behavior`, such as
`[behavior.tool.file]`. Every start reads it again and a move carries it.
A standalone specification setting `[harness]` selects that harness in setup.

### What Link rewrites

Link rewrites only `[access]`, through [`lnk agent share`](../../../../../../docs/agents/connections/connect/share/README.md#share-it-with-a-chat) and `unshare`,
the rest of the file and its comments kept: what a machine overrides is
shown, not written back.

### Never a grant of what it reaches

What the agent may reach is its sandbox file beside it, `sandbox.toml`, written by `lnk agent start --sandbox` or [`lnk agent permission add`](../../../../../../docs/security/sandbox/permissions/README.md#choose-what-your-agent-can-reach): [a specification repeats
nothing of it](../../../../../../docs/security/sandbox/permissions/README.md#choose-what-your-agent-can-reach).
A specification's `[access]` says only [who may ask it](../../../../../identity/docs/config/specs/access.md), and
whoever may can make it do all its sandbox file allows.

### A team's

`~/Link/Safe/Teams/<team>.toml`, [the opinions its members share](../../../../../../docs/config/specs/team.md) beneath
their own.

## More about it

Why it works this way:
[decisions.md](../../../../../../../../../docs/config/specs/decisions.md#where-are-an-agents-opinions).
Every specification Link reads, side by side: [Specifications](../../../../../../../../../docs/config/README.md).

<!-- spec-reference: agent -->
