# Command Aliases

Command aliases let you keep familiar spellings when coming from
another harness. Each alias resolves to an existing namespace command;
the target owns the behavior and receives the arguments unchanged.

## Declared aliases

This is the complete alias list declared for the command surface in
the agent and model registries consumed by Link Harness. Each mapping
links to its declaration and the [namespace command list](namespace-commands.md)
provides the primary names. Availability follows the target, so an
alias grants no additional capability and has no independent switch.

<!-- inventory:command-aliases:start -->

- [`/btw` → `/chat:ask`](../../../../../../agent/actions.toml)
- [`/clear` → `/chat:create`](../../../../../../agent/actions.toml)
- [`/compact` → `/chat:compact`](../../../../../../agent/actions.toml)
- [`/context` → `/model:context`](../../../../../../model/actions.toml)
- [`/cost` → `/model:usage`](../../../../../../model/actions.toml)
- [`/export` → `/chat:export`](../../../../../../agent/actions.toml)
- [`/fork` → `/chat:fork`](../../../../../../agent/actions.toml)
- [`/goal` → `/chat:goal`](../../../../../../agent/actions.toml)
- [`/learn` → `/skill:write`](../../../../../../agent/actions.toml)
- [`/loop` → `/cron:create`](../../../../../../agent/actions.toml)
- [`/model` → `/model:set`](../../../../../../model/actions.toml)
- [`/models` → `/model:list`](../../../../../../model/actions.toml)
- [`/new` → `/chat:create`](../../../../../../agent/actions.toml)
- [`/ps` → `/task:list`](../../../../../../agent/actions.toml)
- [`/queue` → `/chat:queue`](../../../../../../agent/actions.toml)
- [`/rename` → `/chat:rename`](../../../../../../agent/actions.toml)
- [`/resume` → `/chat:resume`](../../../../../../agent/actions.toml)
- [`/side` → `/chat:ask`](../../../../../../agent/actions.toml)
- [`/skills` → `/skill:list`](../../../../../../agent/actions.toml)
- [`/status` → `/chat:status`](../../../../../../agent/actions.toml)
- [`/steer` → `/chat:steer`](../../../../../../agent/actions.toml)
- [`/stop` → `/chat:stop`](../../../../../../agent/actions.toml)
- [`/tasks` → `/task:list`](../../../../../../agent/actions.toml)
- [`/tools` → `/tool:list`](../../../../../../agent/actions.toml)
- [`/usage` → `/model:usage`](../../../../../../model/actions.toml)
- [`/verbose` → `/chat:verbose`](../../../../../../agent/actions.toml)
- [`/whoami` → `/agent:show`](../../../../../../agent/actions.toml)

<!-- inventory:command-aliases:end -->

## Choose a spelling

Use the namespace form in new documentation and automation so the
subject and action stay visible. Use an alias when it matches a command
you already know. For example, `/clear` and `/new` both resolve to
[`/chat:create`](../../../../../../agent/docs/agents/chat/README.md#manage-history); `/model <name>` resolves to `/model:set <name>`.

## Shared execution

Alias lookup selects the same command object as the namespace name;
it has no separate handler. The target's owner checks, argument rules,
specification switch and grants apply to either spelling. An unavailable target
has no usable alias. [Commands](README.md) explains when a slash
message is treated as ordinary text and how each command behaves.

## Other slash names

Root-level `/help` is its own discovery command. It is not an alias.
Skills supply reusable prompts, and their loading and invocation depend
on the harness; see [Skills](../../../../../../agent/docs/agents/tools/use/skill/README.md).
A native command offered by another harness is not automatically a Link
alias. Only the mappings declared above belong to this inventory.
