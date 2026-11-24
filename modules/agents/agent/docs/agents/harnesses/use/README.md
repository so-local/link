# Switch Harnesses

> Switch Harnesses, Keep Your Agent

Switch the harness that runs your agent with `lnk agent harness set <harness>`.
Your agent keeps its model, files, and connections, and Local Link installs the chosen harness for you.

## Switch in One Command

`lnk agent harness list` lists the harnesses and which one runs your agent. `lnk agent harness set <harness>` switches to one, installing the harness the first
time. The list also points to [`lnk mod add --url <url>`](../../../../../../../docs/more/modules/guide.md#add-or-remove-a-module) for a harness
from another repository.

```sh
lnk agent harness list
lnk agent harness set hermes|link
```

### Choose Mini

Choose [Mini Harness](../../../../../harnesses/mini/docs/agents/harnesses/more/mini/README.md) for a small text-and-tools loop with saved conversations. The selection name is `mini`, and Local Link installs the `lnk-mini-harness` program.

```sh
lnk agent harness set mini
```

Every harness Local Link comes with creates fresh, migrates an existing installation, and transfers state in its own way. [Compare Harnesses](../../../../../../../interop/agents/harnesses/docs/agents/harnesses/compare.md) separates the three.

## Understand the Agent Protocol

The [Agent Protocol](../../../../../harnesses/portability/contract/docs/agents/harnesses/more/agent-protocol.md)
explains the common conversation and state interface behind a switch.
Local Link compares the adapters' declared capabilities, captures supported
state, and starts the chosen harness through its adapter. Messages wait
for supported history to be imported before the destination receives
them. The agent's shared files stay available on this machine.

### Inspect Portable State

[Export Agent State](state/README.md) captures portable state for inspection, including native execution evidence when the harness supplies it, without switching harnesses.

## What Comes Along

Your agent keeps its model, key, files folder, and connections. If the new
harness doesn't speak one of your connections, `use` says so, and that
connection stays quiet until you switch back. Each harness's own state
stays in its home, and a copy comes into your agent's folder.

### Carry Your Conversations

For Link Harness, Hermes, OpenClaw, and DeepSeek, `use` asks the harness
it leaves for its state over the Agent State Protocol (ASP), and keeps
the state in your agent's folder (`harnesses/<name>.json`). Once the new
harness answers, it imports the conversations that went on elsewhere
since it last ran, and each chat follows its thread there. A message
that arrives meanwhile, typed or held by a channel during the switch,
waits for them.

#### Wait Out a Harness's Install

The new harness's install and start, minutes for Hermes, count against
no deadline: it has five minutes from when it first answers to take the
conversations in. A failed or expired transfer refuses messages until a
successful retry, so a timeout cannot start an empty conversation.

#### Carry Non-Text State

Switching compares full Agent Client Protocol (ACP) updates, including tools, thinking, and media.
Link Harness and Hermes keep imported updates exactly for export, with separate
native records for text, completed tool calls and supported media. Neither
executes a historical tool call. Native thinking and stored media are
included in their exports. [Link Harness's import](../../../../../harnesses/link/docs/agents/link/more/acp/README.md#imported-conversations) describes the endpoint behavior.

#### Agree to Named Omissions

Session support alone promises text. Before switching, `use` names the
non-text content the destination cannot use and asks for your agreement. In a script, it
requires `--yes`. Exact source updates stay in each harness's
snapshot. Thinking without a destination's native signatures, audio and
unsupported session metadata remain archival in Link Harness.

### Switch Back and Forth

Switching back finds the harness's own state in its home. Link Harness
and Hermes import portable messages into native history. OpenClaw and
DeepSeek retain the complete portable archive and supply bounded historical
context with the next turn. This continuation keeps existing native bindings
and reports the native state it cannot restore. Mini transfers user and
assistant text.

#### Bounded Continuation

Portable context keeps the user's intent and corrections, current memory
revisions, and recent replies and tool evidence that fit. It names omitted
updates and warns that historical evidence may be stale. If required user
turns and memory exceed the budget, continuation is refused and the archive
remains intact. A failed native resume also preserves its original binding.

#### Conversations That Diverged

When the same thread continued separately in two harnesses, Local Link keeps
both versions rather than merging their messages. The destination's version
keeps the thread's name. The version brought in goes on a thread beside
it named for the harness it came from, `link:main~hermes`. Both
`lnk agent chat list` and `lnk agent chat show` reach it. Every harness
that takes conversations in does the same, and `use` names each such
thread. The destination does not import a conversation it already holds again. A conversation that only went
on in the other harness goes on in the destination's thread.

#### Switch from a Stopped Harness

Hermes, OpenClaw, and DeepSeek can export their native files and portable
archive while stopped. Their programs read those files, so `use` first installs a harness never
started here, such as one an agent was migrated onto, as a
start would. A harness without offline export uses its last
kept copy and says so. A failed live capture refuses the switch and
retains the running source.

#### One Running in a Terminal

A switch while your agent runs in another terminal reads the harness's
state first, then stops the agent there as `lnk agent stop` would. The new
harness starts in the terminal that ran `use`, or as a service without
`--foreground`.

### Who Your Agent Is

Who your agent is comes along both ways. Your agent's persona, owner, standing
instructions, and skills go into Hermes's and OpenClaw's own files at
each start. When you switch away from either, the persona, instructions,
and skills made there come into your agent's folder wherever it has
none, and `use` says what came.

### See What Stays Behind

Before the switch, `use` lists what the new harness won't carry. The list holds the
old harness's conversations, unless the old one gives them and the new one takes them
in. It holds the old harness's own memory files,
unless both carry who your agent is. It holds skills and persona, unless
both harnesses read them from your agent's specification. It also holds what your agent's
specification turns on that the new harness doesn't do. Answer no
and nothing changes. In a script, add `--yes`. What stays behind is
there again when you switch back.

#### MCP Servers You Added to a Harness

A switch carries the tools your agent's specification gives it, never the MCP
servers you added to a harness's own settings. Once the old harness
gives its state, `use` names each such server, and each stays with that
harness. Add a server to your agent's specification to use it on every harness.

### Recover from a Failed Switch

A switch that fails once the old harness has stopped starts your agent
again on the old harness, so the agent answers where it did.

#### Retry a History Transfer

A failed history import keeps incoming messages blocked and retains the
source archive. Retry `lnk agent harness set <harness>` for the same
destination after fixing the reported error. Only an acknowledged import
of every requested conversation clears the transfer barrier.

## Allow What a Harness Asks For

A harness that needs more than Local Link's defaults asks before your agent
switches. A harness might want the network without the filtering proxy, for
example, and it says what and why. Answer no and nothing changes.
[Permissions](harness-permissions/README.md#allow-what-a-harness-needs) lists what harnesses ask for, and how to take a grant back.

## Bring an Agent You Already Have

[Migrate Harnesses](migrate/README.md) brings an existing OpenClaw, Hermes, or DeepSeek
agent under Local Link, with a plan to review before the agent's files and secrets are copied.

## Use Another Harness

Four harnesses besides Link Harness come with Local Link, and `lnk agent harness set`
installs each the first time. Native adapters download their own runtime
and ask first, saying what they download. In a script, add `--yes`.
Mini runs its own executable without a separate Node or Python runtime.

### Switch to a Native Harness

Choose the harness you want. The four are alternatives for the same agent, not steps to run in sequence.

```sh
lnk agent harness set openclaw|hermes|deepseek
```

| Harness | What it is |
|---|---|
| [Mini Harness](../../../../../harnesses/mini/docs/agents/harnesses/more/mini/README.md) | Link's small text-and-tools loop |
| [OpenClaw](../../../../../../../interop/agents/harnesses/openclaw/docs/agents/harnesses/openclaw/README.md) | The personal AI assistant, from [openclaw.ai](https://openclaw.ai) |
| [Hermes Agent](../../../../../../../interop/agents/harnesses/hermes/docs/agents/harnesses/hermes/README.md) | [Nous Research](https://hermes-agent.nousresearch.com)'s agent |
| [DeepSeek Harness](../../../../../../../interop/agents/harnesses/deepseek/docs/agents/harnesses/deepseek/README.md) | DeepSeek's [open-source agent](https://github.com/deepseek-ai/deepseek-harness), `dsh` |

Each page says how the harness runs, its limits, and what to do when it
doesn't answer. [`lnk upgrade`](../../../../../../../docs/more/modules/guide.md#upgrade) keeps all seven up to date with Local Link.

### Add a Harness from Outside Local Link

Any other harness comes from outside Local Link, as a module from its own
repository, signed by its own key, that answers
[the harness contract](../../../../../harnesses/portability/contract/docs/agents/harnesses/more/protocol/README.md#harness-programs-lnk-name).
[Add it once](../../../../../../../docs/more/modules/guide.md#add-a-module-from-outside-link), then switch to it by its name.
The URL below is a placeholder. Replace it with your harness module's
repository, which must publish its signed release for your platform.

```sh
lnk mod add --url https://github.com/example/lnk-example
```

### Contribute Here

An adapter for another harness lets any agent switch to it with one
command, carrying its conversations and schedules along. A harness
adapter is a module that answers
[the harness contract](../../../../../harnesses/portability/contract/docs/agents/harnesses/more/protocol/README.md#harness-programs-lnk-name),
as the section above explains.

## Remove a Harness

Delete a harness you no longer use, after `show` has told you what the harness
holds.

```sh
lnk agent harness show --harness openclaw
lnk agent harness delete --harness openclaw
```

`delete` removes the harness's install, settings, memory, and
conversations, and whatever `show` finds of the harness outside its home. It
also deletes the harness's grants, unless another agent here runs the harness. If you add the harness
again, it asks again for what it needs. Your files folder and your
agent's settings stay. When a switch goes wrong, see
[Troubleshooting](troubleshooting.md).

## Keep Schedules Through a Switch

Local Link imports native schedules exported by OpenClaw or Hermes into the
same specification entries that migration creates. The native source is stopped
and the managed copy is cleared before Local Link activates those definitions.
Switching back uses those entries without writing native cron jobs.

### Repeat a Switch Safely

A repeated switch or interrupted import binds each source job to the
same stable schedule identity. The display name is not the identity.
Owner edits to an imported definition survive a later native export.
Each firing has a separate occurrence reference, retained across delivery
retries, and reaches the agent as input from a schedule. A scheduled firing has no
authority to act as an owner message or answer an owner question.

## Understand Switching Details

The trust placed in a harness, and the sandbox around the harness, are in [Security](security.md). Adapter configuration and state transfer are in [Decisions](decisions.md). Incompatible harnesses and denied requirements are in [Troubleshooting](troubleshooting.md).

## Manage the Installation

Inspect installed harnesses, change shared grants, run a sandbox diagnostic, and review what removal deletes with [Manage a Harness](harness/README.md).

<!-- docs-tree:start -->

## Docs

### Pages

- [Manage a Harness](harness/README.md): `lnk agent harness` inspects a harness's installation, permissions, and sandbox for the selected agent
- [Migrate Harnesses](migrate/README.md): Bring an agent you already run with OpenClaw, Hermes, or DeepSeek under Local Link
- [Export Agent State](state/README.md): Capture your agent's portable state for inspection without starting a model request
- [Harness Permissions](harness-permissions/README.md): A harness holds what it needs beyond your agent's grants for itself

### More

- [Decisions](decisions.md): Why harness selection, installation and compatibility work this way
- [Security](security.md): What a harness and its installer can reach, and what remains protected when you switch harnesses
- [Troubleshooting](troubleshooting.md): What to do when switching harnesses goes wrong, one symptom to a heading

<!-- docs-tree:end -->
