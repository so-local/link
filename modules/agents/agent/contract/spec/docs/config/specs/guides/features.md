# Features

What an agent can do, each feature by name, switched by its own keys of
the [agent specification](../../../../../../docs/config/specs/agent.md).

## Every feature

A feature is on where its keys turn it on, and off where a key is left
out, `"none"`, `"never"` or false. Link's specification enables its general features
and leaves every tool off: file, scheduler, run, code and Link MCP. A key of `[connections]` is on
too where a connection's own table turns it on (`[connection.slack] images`).

### Read at each message

Link Harness reads its features from the specification at each message: the
prompt's sections, its commands, the images and documents it's sent, and its
fallback. Link starts the tools as the specification says, at each start.

## Its prompt and commands

What the agent is told, and what it does with a command of yours.
The prompt features below require `[instructions] source = "link"`.
Selecting a file or `"none"` turns off automatic repository, persona
and skills sections without disabling commands or tools.

### `repo-instructions`

The repository's `AGENTS.md`, else `CLAUDE.md`, in its prompt: `[instructions] repository`.

### `persona` and `skills`

Who it is and who you are, apart from any task, `[instructions] persona`
or `owner`, and each skill's name and what it's for, `[instructions]
skills`.

### `commands`

A message of yours that runs `/help`, a chat command, a model command, a skill command or [`/skill:write`](../../../../../../../harnesses/link/docs/agents/harnesses/link/commands/skills.md#learn-a-skill): `[commands] help`, `status`, `create`, `stop`, `resume`, `fork`, `rename`, `export`, `queue`, `steer`, `compact`, `ask`, `verbose`, `model`, `skills` and `write`. The commands are these:

- Chat commands: [`/chat:status`](../../../../../../../harnesses/link/docs/agents/harnesses/link/commands/README.md#inspect-its-work), [`/chat:create`](../../../../../../../harnesses/link/docs/agents/harnesses/link/commands/README.md#start-a-new-conversation), [`/chat:stop`](../../../../../../../harnesses/link/docs/agents/harnesses/link/commands/README.md#stop-an-answer), [`/chat:resume`](../../../../../../../harnesses/link/docs/agents/harnesses/link/commands/chats.md#resume-an-earlier-one), [`/chat:fork`](../../../../../../../harnesses/link/docs/agents/harnesses/link/commands/chats.md#fork-this-one), [`/chat:rename`](../../../../../../../harnesses/link/docs/agents/harnesses/link/commands/chats.md#name-this-one), [`/chat:export`](../../../../../../../harnesses/link/docs/agents/harnesses/link/commands/chats.md#write-one-out), [`/chat:queue`](../../../../../../../harnesses/link/docs/agents/harnesses/link/commands/chats.md#queue-or-steer-your-next-message), [`/chat:steer`](../../../../../../../harnesses/link/docs/agents/harnesses/link/commands/chats.md#queue-or-steer-your-next-message), [`/chat:compact`](../../../../../../../harnesses/link/docs/agents/harnesses/link/commands/chats.md#compact-it-now), [`/chat:ask`](../../../../../../../harnesses/link/docs/agents/harnesses/link/commands/chats.md#ask-on-the-side) and [`/chat:verbose`](../../../../../../../harnesses/link/docs/agents/harnesses/link/commands/chats.md#show-its-tool-calls).
- Model commands: [`/model:show`](../../../../../../../model/docs/agents/models/guide.md#where-things-live), [`/model:list`](../../../../../../../model/docs/agents/models/README.md#list) and [`/model:set`](../../../../../../../harnesses/link/docs/agents/harnesses/link/commands/models.md#switch-this-conversation).
- Skill commands: [`/skill:list`](../../../../../../../harnesses/link/docs/agents/harnesses/link/commands/skills.md#run-a-skill) and `/skill:<name>`.

### `goals`

[`/chat:goal`](../../../../../../../harnesses/link/docs/agents/harnesses/link/commands/README.md#pursue-a-goal): a goal it pursues after each answer until it holds, as many
turns as `[goals] turns` lets it, while `[commands] goal` is on.

### `model-fallback`

[`lnk agent start --model.fallback_model`](../../../../../../docs/agents/README.md#start-your-agent)'s model, for a turn its own can't answer: `[model] fallback`. A fallback is a hosted model: a specification whose `fallback_model` names one on this machine (`local/...`, or an alias of one) is refused, with falling back switched on or off.

## Its messages

What of a message it takes, and how its answer is shown.

### `images` and `attachments`

A message's images, `[connections] images`, and its documents, `[connections] documents`.

### `voice` and `groups`

A message's voice notes, heard as their words by `[model.<provider>]
transcribe`, `[connections] voice`; and group chats, answering when
mentioned, `[connections] groups`.

### `streaming` and `activity`

An answer shown as it's written, `[connections] stream`, and with `activity` too, each tool it calls shown above the answer being written, as it calls it, with what it works on. `activity` is `replace`, Link's, `keep`, which leaves the lines above the finished answer, or `off`.

## Its tools

Link's tools are each a feature or more. Every tool starts off in Link's defaults: file, scheduler, run, code and Link MCP. An agent turns on each one it uses.
Add each of those explicitly with [`lnk agent tool add <name>`](../../../../../../docs/agents/README.md#tools) or set
its `[tool.<name>] enabled = true` opinion.

### `subagents`, `schedules` and `proactive`

The scheduler: subagents, schedules and messages it sends first, `[tool.scheduler] enabled`.

### `shell`, `file-read` and `file-edit`

The run tool, `[tool.run] enabled`, and the file tool, reading and
editing, `[tool.file] enabled`.

### `code-search`, `diffs` and `lsp`

The file tool's `find`, `[tool.file] enabled` and `find`; a `write` or
`replace` answered with its diff, `[tool.file] enabled` and `diffs`;
and the code tool's language servers, `[tool.code] enabled`.

## New agents

[`lnk agent create`](../../../../../../docs/agents/start/create.md#create-on-a-hosting-account) and the first `lnk agent start` write Link's
specification out as concrete defaults under `[behavior]` in
`~/Link/Safe/Agents/<agent>/agent.toml`, with every tool off,
and no `extends` line. Turn a feature off a key at a time, as this
agent working in a repository does with its persona, your profile and
group chats:

```toml
schema = 1
[instructions]
persona = "none"
owner = "none"

[connections]
groups = "never"
```

[`lnk agent spec show`](../../../../../../docs/config/specs/agent.md#use-it) marks each key with whose it is: `Link's`,
a team's or `yours`.

## What a harness honors

[Each harness's contract](../../../../../../../harnesses/portability/contract/docs/agents/harnesses/more/protocol/contract.md#features) lists the features it honors, each as the specification
says or its own way.
`lnk agent start` refuses a specification whose own keys turn on a feature its harness doesn't honor, naming the feature and its key,
before anything starts. A feature Link's specification alone turns on is never
refused: the harness does what it can of it.

<!-- spec-reference: agent -->
