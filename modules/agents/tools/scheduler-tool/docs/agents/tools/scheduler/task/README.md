# Task Actions

Use `lnk agent task` to delegate work or run a command through an agent’s enabled providers. These terminal actions use the same `task` namespace as the agent’s tools and retain each provider’s grants and limits.

## Supply a Request

Pass the action’s JSON arguments in a file with `--request <file>`, or on standard input. `--agent work` selects another agent, and `--json` requests machine-readable output. Conversation-scoped work also needs its `thread` in the JSON request, such as `"thread": "link:main"`. A terminal action has no ambient chat. Enabling a provider is a separate step in the [Scheduler guide](../guide.md) or [Run guide](../../../../../../run-tool/docs/agents/tools/run/guide.md).

## Delegate Work

`lnk agent task agent` starts a delegated task. In Link Harness, `/task:agent` accepts the corresponding JSON request in
the current chat. Read about delegated work's limits, conversation ownership, and answer destinations in the [Scheduler guide](../guide.md). To schedule a prompt instead, use `lnk agent cron`, described in [Cron Actions](../cron/README.md).

## Run Commands

`lnk agent task run` runs a shell command and waits for the command to finish. With `run_in_background` in the request, the command starts in the background instead. Shell, filesystem, network, timeout, and background-lifetime behavior belong to [Run](../../../../../../run-tool/docs/agents/tools/run/guide.md).

## Inspect or Stop Work

In Link Harness, `/task:list`, `/task:read`, and `/task:stop` invoke these
operations in the current chat. `lnk agent task list`, `lnk agent task read`, and `lnk agent task stop` inspect or stop work from the current conversation. `lnk agent task list --all` lists every conversation's scheduled work and subagents, which only your command line may ask for. When both providers are available, `kind = "run"` selects command work and `kind = "schedule"` selects scheduler work. A combined list can include both. Reading or stopping an ambiguous identifier requires the kind.
