# Namespace Commands

Namespace commands are the primary names for an agent's actions. Use
these names to learn Link's capabilities and write examples whose
subject and operation are explicit, such as `/task:list`.

## Declared commands

This list covers every action with the `command` surface in the agent
and model registries consumed by Link Harness. Each entry links to its
owning declaration. It includes actions requiring a provider or grant;
`/help` shows the subset available to the current agent. Root-level
`/help` is a discovery command outside these namespaces.

<!-- inventory:namespace-commands:start -->

- [`/agent:send`](../../../../../../../agent/actions.toml)
- [`/agent:show`](../../../../../../../agent/actions.toml)
- [`/browser:click`](../../../../../../../agent/actions.toml)
- [`/browser:close`](../../../../../../../agent/actions.toml)
- [`/browser:open`](../../../../../../../agent/actions.toml)
- [`/browser:read`](../../../../../../../agent/actions.toml)
- [`/browser:screenshot`](../../../../../../../agent/actions.toml)
- [`/browser:scroll`](../../../../../../../agent/actions.toml)
- [`/browser:select`](../../../../../../../agent/actions.toml)
- [`/browser:type`](../../../../../../../agent/actions.toml)
- [`/chat:ask`](../../../../../../../agent/actions.toml)
- [`/chat:compact`](../../../../../../../agent/actions.toml)
- [`/chat:create`](../../../../../../../agent/actions.toml)
- [`/chat:export`](../../../../../../../agent/actions.toml)
- [`/chat:fork`](../../../../../../../agent/actions.toml)
- [`/chat:goal`](../../../../../../../agent/actions.toml)
- [`/chat:queue`](../../../../../../../agent/actions.toml)
- [`/chat:rename`](../../../../../../../agent/actions.toml)
- [`/chat:resume`](../../../../../../../agent/actions.toml)
- [`/chat:status`](../../../../../../../agent/actions.toml)
- [`/chat:steer`](../../../../../../../agent/actions.toml)
- [`/chat:stop`](../../../../../../../agent/actions.toml)
- [`/chat:verbose`](../../../../../../../agent/actions.toml)
- [`/cron:create`](../../../../../../../agent/actions.toml)
- [`/cron:delete`](../../../../../../../agent/actions.toml)
- [`/cron:list`](../../../../../../../agent/actions.toml)
- [`/desktop:click`](../../../../../../../agent/actions.toml)
- [`/desktop:drag`](../../../../../../../agent/actions.toml)
- [`/desktop:key`](../../../../../../../agent/actions.toml)
- [`/desktop:screenshot`](../../../../../../../agent/actions.toml)
- [`/desktop:scroll`](../../../../../../../agent/actions.toml)
- [`/desktop:type`](../../../../../../../agent/actions.toml)
- [`/file:definition`](../../../../../../../agent/actions.toml)
- [`/file:diagnostics`](../../../../../../../agent/actions.toml)
- [`/file:edit`](../../../../../../../agent/actions.toml)
- [`/file:glob`](../../../../../../../agent/actions.toml)
- [`/file:grep`](../../../../../../../agent/actions.toml)
- [`/file:read`](../../../../../../../agent/actions.toml)
- [`/file:references`](../../../../../../../agent/actions.toml)
- [`/file:write`](../../../../../../../agent/actions.toml)
- [`/memory:links`](../../../../../../../agent/actions.toml)
- [`/memory:list`](../../../../../../../agent/actions.toml)
- [`/memory:read`](../../../../../../../agent/actions.toml)
- [`/memory:remember`](../../../../../../../agent/actions.toml)
- [`/memory:search`](../../../../../../../agent/actions.toml)
- [`/memory:show`](../../../../../../../agent/actions.toml)
- [`/model:chat`](../../../../../../../model/actions.toml)
- [`/model:context`](../../../../../../../model/actions.toml)
- [`/model:count`](../../../../../../../model/actions.toml)
- [`/model:embed`](../../../../../../../model/actions.toml)
- [`/model:get`](../../../../../../../model/actions.toml)
- [`/model:help`](../../../../../../../model/actions.toml)
- [`/model:list`](../../../../../../../model/actions.toml)
- [`/model:set`](../../../../../../../model/actions.toml)
- [`/model:show`](../../../../../../../model/actions.toml)
- [`/model:transcribe`](../../../../../../../model/actions.toml)
- [`/model:usage`](../../../../../../../model/actions.toml)
- [`/skill:list`](../../../../../../../agent/actions.toml)
- [`/skill:write`](../../../../../../../agent/actions.toml)
- [`/task:agent`](../../../../../../../agent/actions.toml)
- [`/task:list`](../../../../../../../agent/actions.toml)
- [`/task:read`](../../../../../../../agent/actions.toml)
- [`/task:run`](../../../../../../../agent/actions.toml)
- [`/task:stop`](../../../../../../../agent/actions.toml)
- [`/tool:list`](../../../../../../../agent/actions.toml)
- [`/web:fetch`](../../../../../../../agent/actions.toml)
- [`/web:search`](../../../../../../../agent/actions.toml)

<!-- inventory:namespace-commands:end -->

## Arguments and availability

Link Harness handles conversation controls and model selection locally,
as described in [Commands](README.md). Actions reached through a route
take a JSON object after their name, using that action's request
contract. A declaration alone does not guarantee a working provider;
the agent's grants, settings and connected providers determine what
runs. See [Action Namespaces](../../../../../../../agent/docs/agents/more/interface/namespaces.md).

## Familiar spellings

[Command Aliases](aliases.md) lists the compatibility spellings that
resolve to these commands. An alias uses its target's implementation,
arguments and availability rules. Native commands offered by another
harness belong to that harness and are outside this inventory.
