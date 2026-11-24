# Link Harness Guide

Configure Link Harness’s instructions, skills, tools, and model fallback, then inspect the prompt and conversation it keeps. Its editable files and agent specification control each turn without requiring a separate harness runtime.

```sh
lnk agent start
lnk agent harness set link
lnk agent chat send
```

## Choose Link Harness

Link Harness comes with Link as a Rust executable, and installs no
separate Node or Python harness runtime. It builds the prompt from your
files, calls your chosen model and tools, and saves the conversation
between turns. Your model still determines answer quality and much of
the response time; [Measure](../../../../../../tooling/measure/docs/tooling/measure/README.md)
shows the time and resources your own workload uses.

### Install the module

After installing Link, you can install the harness module separately:

```sh
lnk mod add harness
```

### What it needs

Use a Mac or Linux computer with the agent modules and a model. [Agents](../../../../../agent/docs/agents/start/README.md#check-what-starting-an-agent-needs)
owns setup and costs. A hosted model bills you through its provider;
a model running on your computer uses your computer's resources.

### Use a model

Use the same [model selection](../../../../../agent/docs/agents/models/agent/README.md)
as other Link agents. Link Harness accepts local runtimes discovered
by Link and hosted models through Link's proxy. A model must support
tool calls to use tools, memory and skills through memory.

### The trade-offs

You get visible prompt sections, file-based instructions and skills,
configurable conversation storage and a hosted fallback. You also take
on choosing an appropriate model and setting its limits. Another harness
may suit an established workflow that depends on its own prompt,
extensions or native state. [Compare Harnesses](../../../../../../../interop/agents/harnesses/docs/agents/harnesses/compare.md)
and [Switch Harnesses](../../../../../agent/docs/agents/harnesses/use/README.md)
explain the alternatives and transfer boundaries.

## Switch to it, and back

Link keeps your model, files and connections while switching. Link
Harness imports and exports conversations through the Agent State Protocol (ASP), so conversations can continue when the other harness supports
the matching transfer. Link names anything the destination cannot carry
before switching: [what comes along](../../../../../agent/docs/agents/harnesses/use/README.md#what-comes-along).

```sh
lnk agent harness set link
lnk agent harness set hermes
```

## Make it yours

The agent owns its editable instruction and skill files; Link Harness
reads them to build each turn's prompt. Its [specification](../../../../../agent/docs/config/specs/agent.md)
controls the sources and size limits, which you can inspect alongside
the resulting prompt.

### Tell it who to be

Edit standing instructions, persona and owner information in the
agent's instructions folder, `~/Link/Agents/main/agent/instructions/`.
Link Harness reads the files at each message, so you can change them
without restarting it. `instruction.md` holds what it always does.

```markdown
Answer in French, in one sentence.
```

`persona.md` holds who it is.

```markdown
You are Ada: dry, warm, and never in a hurry.
```

`owner.md` holds who you are.

```markdown
Sam Okafor, a night-shift nurse in Leeds; sister Maya, cat Biscuit.
```

Inspect the assembled prompt to verify the change.

```sh
lnk agent show --instructions --json
```

### Use one Markdown file

Set `source` in the agent's [specification](../../../../../agent/docs/config/specs/agent.md)
to use one file as its whole system prompt. Link Harness reads it at
each turn without adding sections, headings, model-specific text or
message timestamps. You control its contents and their order.

```toml
schema = 1
[instructions]
source = "instructions/instruction.md"
```

#### Inspect the file prompt

Edit `agent/instructions/instruction.md` and inspect the result below.
The storage reader trims outer whitespace and reads at most 64 KB.
A missing, empty or linked file gives an empty system prompt. Set
`source = "link"` to restore the assembly, or `"none"` for no system
instructions. Tool definitions and conversation history still reach
the model in either mode.

```sh
lnk agent show --instructions --json
```

### Use Link's assembly

The default assembly groups instructions into sections and orders
them for caching. Its section settings remain available while a file
is selected, ready for switching back to `source = "link"`.

#### Its prompt's sections

With the default `source = "link"`, its prompt is made of sections, in the order of how often each changes,
least first, so the provider's cache holds up to the first that changed:

1. **Base**: Link's (a personal agent who answers briefly), a file of
   yours, or none. It changes with a release.
2. **Model**: yours for the model it runs on,
   `instructions/models/<maker>/<model>/instruction.md` in its files,
   the model named as OpenRouter names it. It changes when you edit it,
   or it runs another model.
3. **Tools**: what each tool it's offered says of itself as it starts
   (MCP's `instructions`), under the tool's name. It changes at a start.
4. **Environment**: where its files are, which of their folders it may
   change, its teams and teammates, the connections it's reached on,
   and, with no `run`, that it can't run programs. It changes at a
   start.
5. **Repository**: in the repository it works in, each folder's
   `AGENTS.md`, else its `CLAUDE.md`, from Home up to the repository's
   root, the nearest first. It changes when one is edited.
6. **Skills**: each skill's name and what it's for, from
   `skills/<name>/SKILL.md`. It changes when one is added.
7. **Persona**: `instructions/persona.md`, who it is whatever the task:
   its name, its voice, its character. It changes when you edit it.
8. **Owner**: `instructions/owner.md`, who you are: your name, your
   work, the people in your life, how you like things. It changes when
   you edit it.
9. **Standing**: `instructions/instruction.md`, what you tell it to do.
   It changes when you edit it.
10. **Notes**: `memory/notes.md`, what it noted for itself. It changes
    when it notes something.
11. **Now**: the date, local time, connection and speaker, attached
    to each message rather than the system prompt.

#### Who wrote each file

In the assembly, persona, owner and standing are files in its own folder, each headed
with who wrote it: you, or you or it where it may write
`instructions/`.

#### Now

Now is never in the prompt: each message starts with it
(`[Thursday 8 October 2026, 14:03 (UTC+02:00), on Telegram, from your
owner]`), kept with the message so it reads the same at every turn
after.

#### A message from anyone else

A message from anyone else says who on the line after, each of its
lines quoted (`> `) so none reads as yours: a schedule, a subagent,
another agent, or someone in a chat you shared it with, by their name
there and their connection (`(from "Alice Ng" on Slack, in a chat your
owner shared you with, not your owner:)`).

#### Where each section comes from

Your [specification](../../../../../agent/docs/config/specs/agent.md)'s
`[instructions]` says where each section comes from, Link's or a file
of yours, or leaves it out, and `[instructions.max]` how large each may
be, cutting what's past it. [`lnk agent show --instructions --json`](../../../../../agent/docs/agents/models/agent/README.md#set-its-model-and-how-hard-it-thinks) shows each
section, where it came from and its size.

#### The repository's files

The repository's root is the nearest folder holding `.git`: outside a
repository, or where its sandbox hides the root, Home's file alone is
read, and a file its sandbox doesn't let it read, or a link, is left
out.

#### A model's file

A model's file is named as [OpenRouter](https://openrouter.ai/models)
names the model, its maker then its name:
`instructions/models/anthropic/claude-opus-5-5/instruction.md` for
`claude-opus-5-5` on Anthropic,
`instructions/models/openai/gpt-6-luna/instruction.md` on OpenAI. A
model through OpenRouter is named so already
(`anthropic/claude-opus-5.5`), so its folder is its name.

#### A model on a runtime here

One on a runtime here goes under the runtime's
(`instructions/models/ollama/qwen3:8b/instruction.md`). `lnk agent show --instructions --json` says the path for the model it runs now. A turn
its [fallback](#fall-back-to-a-hosted-model) answers is sent the
fallback's file in its place. No specification key names it: the folder is the
only place.

### Its folder

The agent's [folder](../../../../../agent/docs/agents/more/settings/README.md#an-agents-folder)
holds `home/` for your documents and `agent/` for instructions,
skills, conversations, notes and schedules. Edit instructions and
skills there as plain files. The [file permissions](../../../../../agent/docs/security/sandbox/permissions/README.md#choose-what-your-agent-can-reach)
decide what the agent itself may change; its tools cannot rewrite its
conversation log.

### Change its opinions

Inspect [`lnk agent spec show`](../../../../../agent/docs/config/specs/agent.md#use-it) to see the context window, reasoning effort,
round limits and tool-result limits Link Harness reads. Their types,
defaults and values live in the [Agent Specification](../../../../../agent/docs/config/specs/agent.md),
so the harness and your configuration share one source of settings.

### Choose its features

Turn each [feature](../../../../../agent/contract/spec/docs/config/specs/guides/features.md)
on or off by its own keys. Link Harness reads the resulting feature
settings at each message, including whether repository instructions,
persona and group-chat features are included.

### Give it memory and tools

Add tools through [Agents](../../../../../agent/docs/agents/tools/use/README.md#give-it-tools).
Link Harness receives them as MCP tools through Link's proxy and offers
their descriptions to the model. Their own pages explain [memory](../../../../../tools/memory-tool/docs/agents/tools/memory/README.md),
[web access](../../../../../tools/web-tool/docs/agents/tools/web/README.md),
[programs](../../../../../tools/run-tool/docs/agents/tools/run/README.md)
and [files](../../../../../tools/file-tool/docs/agents/tools/file/README.md).

#### What a tool says of itself

The prompt includes each offered server's MCP `instructions`, under
its name, at most 2 KB per tool server by default. When a conversation
narrows its tools, a server with none offered contributes no instructions.
The model's calls and the tools' replies become conversation events.

#### Its notes

Link Memory's `remember` changes `memory/notes.md`, which Link Harness
reads into every conversation's prompt. The file is editable by you;
its size limit and write permissions are documented with [Memory](../../../../../tools/memory-tool/docs/agents/tools/memory/guide.md#let-it-take-notes).
This gives every conversation the same current notes while its own
message history stays separate.

#### Its limits

A busy tool bridge is retried within the tool's startup deadline.
Transient HTTP 503 responses do not hide a tool for five minutes;
if it remains busy, the next turn can try again. Other startup failures
retain their bounded cooldown. An operation that has started is never
repeated by this startup retry.

#### Turn and call limits

A message makes at most eight rounds of calls by default, then answers
with what it has. A call taking more than five minutes, including
waiting for your approval, is cancelled and the model is told. Both
limits are configurable in the specification. A long program can run in the
background through [Link Run](../../../../../tools/run-tool/docs/agents/tools/run/README.md),
which sends its completion back into the conversation.

#### Order of One Round's Calls

The calls of one round run in the order the model asked. Calls side by
side that only read, such as two file reads, run at once. A call that
changes something waits for the calls before it, and the calls after it
wait for it, so a list asked after a delete sees the delete. The
[decisions](more/decisions.md#in-what-order-do-one-rounds-calls-run)
say how a tool marks a call that only reads.

### Give it skills

Add, edit and remove skills through the agent's [Skills](../../../../../agent/docs/agents/tools/use/skill/README.md)
page. Link Harness lists each skill's name and description in the
prompt, then the model reads the full `SKILL.md` through Link Memory
when needed. Start memory for that reading path.

```sh
lnk agent memory start
```

#### How it uses one

The list is rebuilt from skill metadata at each turn, sorted by name.
It holds the names and descriptions rather than the full instructions,
so installing a skill does not put all its text into every prompt.
The model reads the chosen skill and follows its links when the task
needs them.

#### Room in the prompt

The default `[instructions.max] skills` allows 8 KB for the list.
Skills that do not fit are omitted whole, rather than cut mid-description,
and `lnk agent show --instructions --json` shows the section as `cut`. A skill
with missing or mismatched metadata is omitted. Its scripts run under
the same permissions as other programs.

## How it answers

A turn reads its current instructions and saved history, builds the
model's context, calls the model and tools, and appends the results.
Only one turn runs at a time per conversation. [Conversations](conversations.md)
explains context limits, summaries, forks and crash recovery in detail.

### Fall back to a hosted model

Link Harness can use a hosted fallback for a turn its primary model
cannot answer. Configure it through [Switch Models](../../../../../agent/docs/agents/models/agent/README.md#a-hosted-model-to-fall-back-to),
which owns model selection, credentials and provider restrictions.

```sh
lnk agent start --model.default local/qwen3 --model.fallback_model openrouter/qwen/qwen3-32b
```

#### When it falls back

For one turn at a time, the fallback answers when your model's runtime
is unreachable or sends nothing back within 60 seconds, its provider
answers busy or overloaded, or its prompt still exceeds the model
window after older turns leave it. The turn's later rounds stay with
the fallback, whose model-specific instructions replace the primary
model's instructions.

#### A model found off

A model found off is left alone for 3 minutes, its turns going to the
fallback at once, and the turn after tries it again. Summaries of what
leaves the window are asked the same way.

#### What it says of it

Each answer says which model wrote it, and the turn's measures count it
(`link.fallbacks.off.sum`, `link.fallbacks.window.sum` in `lnk
measure`).

### Talk to it

Connect and share your agent through [Connections](../../../../../connections/docs/agents/connections/README.md),
which owns chat routing, replies, streaming and who may speak to it.
Those messages reach Link Harness through the same Agent Client Protocol (ACP) interface as a terminal client.

#### Its commands and ACP

Send `/help` for harness commands, [`/chat:create`](../../../../../agent/docs/agents/chat/README.md#manage-history) for a fresh conversation
and `/model:set` to change its model: [Commands](commands/README.md).
The [ACP](more/acp/README.md) page documents its client
interface, sessions, cancellation and steering.

#### Send it images and documents

Link Harness reads message photos and documents according to the
agent's feature settings, and stores the attachments with the
conversation: [Images and documents](media/README.md) explains supported
content, size limits and how each provider receives it.

## Its conversations

Conversations persist independently of the harness process. The default
append-only log keeps model messages, tool calls and replies, with a
Markdown copy to read. Compact files and store drivers are alternatives
with different storage and reading costs. [Conversations](conversations.md)
explains their formats, migration, context window and recovery behavior.

## On a machine that packs agents

A machine that runs each agent only while it answers can start Link
Harness ahead, blank: `lnk-harness serve --control <control>` holds
nothing of anyone's and answers on no port, only on the Unix socket
`<control>`, the user's alone.

### Bound to one agent

One line binds it to one agent (its name, ID and owner, a key made for
that binding, its settings, and its bridges' sockets), and from then on
it answers that agent alone, on its usual port, with that key; it keeps
the settings in memory and writes none. It's bound once: a second
binding is refused, and the process ends when the agent sleeps.
[Why](more/decisions.md#why-can-link-harness-start-blank).

## More about it

The pages beside this one, and those on Link's agents that set Link
Harness among the rest:

- [Troubleshooting](troubleshooting.md), when it doesn't answer or
  answers wrong.
- The [`lnk agent`](../../../../../agent/docs/agents/README.md#agents) commands, on the
  [Agents](../../../../../agent/docs/agents/README.md) page.
- [Compare
  Harnesses](../../../../../../../interop/agents/harnesses/docs/agents/harnesses/compare.md),
  Link Harness beside the others.
- [Agents
  Security](../../../../../agent/docs/agents/harnesses/use/security.md#link-harness),
  its guarantees and gaps.
- [Decisions](more/decisions.md), why it works this way, and [Agents
  Decisions](../../../../../agent/docs/agents/harnesses/use/decisions.md#why-does-link-have-a-harness-of-its-own),
  why Link has a harness of its own.
- [Developing](more/developing.md), to work on its code.
