# Hermes Agent Guide

Run Hermes Agent with a hosted or local model, and carry your conversations and memory between harnesses. The adapter manages its runtime, settings, and chat bindings while Link carries the messaging connections.

```sh
lnk agent start
lnk agent harness set hermes
lnk agent connect telegram
```

## Use a hosted model

Anthropic, OpenAI and OpenRouter work with your own key. `lnk agent
start` asks for it, or reads it from the provider's variable, such as
`ANTHROPIC_API_KEY`, when that is set in its environment.

## Use a local model

Use a local model from a runtime [`lnk model list`](../../../../../../../../modules/agents/model/docs/agents/models/README.md#list)
finds and reaches through an OpenAI-compatible endpoint, such as Ollama,
LM Studio, or llama.cpp. The runtime's context must fit Hermes's prompt;
[Troubleshooting](troubleshooting.md) shows an Ollama configuration example.


```sh
lnk up
lnk agent start --model.default local/qwen3
```

## Bring an Existing Hermes Agent

[`lnk agent migrate --harness hermes`](../../../../../../../../modules/agents/agent/docs/agents/README.md#migrate-your-agent)
copies a standalone Hermes installation and its selected profiles into
Link-managed agents. The plan identifies native files, secrets,
connections, and scheduled jobs before changing them. Creating a fresh
agent on Hermes instead uses [`lnk agent create`](../../../../../../../../modules/agents/agent/docs/agents/start/create.md#create-on-a-hosting-account).

## Move from OpenClaw to Hermes

`use` hands Hermes your model and files folder, and Link's bridge carries
your connections to it, the same Telegram bot, Slack app, Discord bot and
Signal account, so it answers the same chats and Hermes itself holds no
bot's credential. Messages sent during a stop follow the connection's
recovery limits; once it runs, [`lnk agent chat send`](../../../../../../../../modules/agents/agent/docs/agents/chat/README.md#talk-to-it-here) reaches it too.

```sh
lnk agent harness set hermes
lnk agent show
lnk agent harness set openclaw
```

### What it carries

The [Agent Protocol](../../../../../../../../modules/agents/harnesses/portability/contract/docs/agents/harnesses/more/agent-protocol.md)
combines conversation control with supported state transfer.
Through the Agent State Protocol (ASP), Hermes exports its
conversations, instructions, skills, and memory. Incoming supported
conversations become native Hermes sessions. Workspace files remain in
the shared agent Home and are not copied by ASP.

### Its memory, exported

Its `MEMORY.md` and `USER.md` are retained as sourced notes when
switching away. Another harness receives the supported memory
representation through its adapter or Link's memory mechanisms.
The [comparison](../../../../../docs/agents/harnesses/compare.md#carry-other-state)
distinguishes those mappings from native memory files that stay with a harness.

### What others remember

At each start, what your agent remembers elsewhere, Link's notes and
what OpenClaw remembered, becomes entries of Hermes's `MEMORY.md`,
each opening `(from link)` or `(from openclaw)`, after its own and
within 1,600 of its 2,200 characters, so its memory tool can still add.
Those entries are never exported back.

#### When it reads them

Hermes caches native memory when a conversation starts. The hub also
supplies changed sourced memory with the next turn through bounded context.
Corrections and deletions update actual native files on return, provided
the native text has not changed since its export. A conflicting edit
refuses configuration until reconciled.

### Each chat's thread

Each chat Link's bridge carries is the thread its session names
(`telegram:42`), on Telegram, Slack, Discord and Signal.

### Who your agent is

Your agent's persona, owner and standing instructions go into a block of
Hermes's `SOUL.md` at each start, the rest of the file as it was, and
its skills are one of Hermes's skills folders.

### Switching away

What `SOUL.md` holds besides, unless it's the one Hermes wrote, and the
skills Hermes made, neither bundled with it nor from its hub, come into
your agent's folder wherever it has none. Native history and memory
files stay in Hermes's home. Hermes's `hermes claw migrate` remains a
separate one-way import that Link does not run.

### State over ASP

Asked over ASP, as a switch asks, Hermes gives and takes its
conversations through its own `SessionDB`, run by its own Python: one
that comes in becomes a session of `hermes acp`'s, once, so it's listed
and goes on. Its `SOUL.md`, its own skills, its memory and the MCP
servers you added to its `config.yaml` are carried both ways; its
settings are Link's.

#### Imported conversations resume

An imported conversation is written as a plain `acp` session, the kind
Hermes's gateway takes up as yours when Link resumes it over the Agent
Client Protocol (ACP). A session Hermes's foreign-session importer made,
which records where it came from, Hermes only shows and refuses to
resume. The session's ID is a UUID made from the thread and its state,
so the same thread imported twice is one session, and a later version
of it another.

### Machine Moves

[`lnk agent move`](../../../../../../../../modules/agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud)
carries the adapter-declared native Hermes state and the agent's shared
files, then installs Hermes for the destination machine. This keeps the
same harness and its native resources, while switching to a different
harness uses portable representations and the destination's import support.

#### Imported sessions

Completed tool records and images become native conversation records without
executing historical effects. Exact imported updates stay in
`.hermes/link-imports.json` and return unchanged on export, with later native
work appended. Divergent native history keeps both branches. Thinking and
unsupported content remain archival, named before switching and in the
import response.

#### Older Native History

Export includes compacted messages and inactive sessions through Hermes's
native database API. Every original message row, including unknown fields,
is retained beside its readable projection. Native compaction and active
flags identify separate row revisions, preserving both originals when
those flags change. Changed content under the same revision refuses
capture instead of silently replacing evidence.

## How it runs

Link gives hermes its own runtime settings and carries each chat through the adapter endpoint. The [runtime reference](more/runtime.md) explains installation, native configuration, and the process lifecycle.

## Inspect Native State

The [runtime reference](more/runtime.md#files-it-keeps) identifies Link's
native tool entries, endpoint key and persistent chat bindings beside
Hermes's own files.

## More about it

Read [Decisions](decisions.md) for the hermes adapter’s design, [Security](security.md) for its execution and credential limits, and [Troubleshooting](troubleshooting.md) for recovery. Compare its model and state support with [other harnesses](../../../../../docs/agents/harnesses/compare.md).

## Work on the Adapter

[Developing](more/developing.md) gives the hermes adapter’s local build checks and license. [Troubleshooting](troubleshooting.md) helps recover its running process.

## Native Schedules

Native schedule definitions are exported for Link’s schedule handoff. The [runtime reference](more/runtime.md#native-schedules) describes revision checks, omissions, and clearing the managed native source.
