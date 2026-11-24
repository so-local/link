# Cron Actions

Use `lnk agent cron` to schedule a prompt for an agent from your terminal, and to list or delete its schedules. These terminal actions use the same `cron` namespace as the agent’s scheduler tool and keep the tool's grants and limits.

## Supply a Request

Pass the action’s JSON arguments in a file with `--request <file>`, or on standard input. `--agent work` selects another agent, and `--json` requests machine-readable output. A schedule belongs to a conversation, which the JSON request names with a `thread`, such as `"thread": "telegram:123456789"`. A request that names none schedules in the agent's main chat, `link:main`. Turning the scheduler on is a separate step in the [Scheduler guide](../guide.md).

## Schedule a Prompt

`lnk agent cron create` schedules a prompt by a cron, recurring or once. In Link Harness, `/cron:create` accepts the same JSON request in the current chat, and `/loop` is its alias. Read about the cron's clock, limits, conversation ownership, and answer destinations in the [Scheduler guide](../guide.md).

## List or Delete Schedules

`lnk agent cron list` lists the agent's schedules, and `lnk agent cron delete` deletes one by its ID. In Link Harness, `/cron:list` and `/cron:delete` invoke these operations in the current chat. Read or stop a firing's work with `lnk agent task`, described in [Task Actions](../task/README.md).
