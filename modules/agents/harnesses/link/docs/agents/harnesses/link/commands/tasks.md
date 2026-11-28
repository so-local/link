# Task Commands

Task commands list and manage the agent's work through its task
namespace: background commands, subagents and schedules.

## List its tasks

[`/task:list`](../../../../../../../tools/scheduler-tool/docs/agents/tools/scheduler/task/README.md#inspect-or-stop-work) lists running and scheduled work through
the agent's task namespace. The command needs that namespace's grant
and `[commands] tasks = true`. It shares the same provider approvals
and combined results as `task__list`. `/task:list --agents` lists the
subagents alone, each with its conversation, `task-<id>`.

## Words for the common cases

The task namespace's commands take their route's JSON, or words for the
common cases, each read into the requests its providers already take.
Each needs the namespace's grant.

### Stop its work

[`/task:stop --all`](../../../../../../../tools/scheduler-tool/docs/agents/tools/scheduler/task/README.md#inspect-or-stop-work) stops the agent's work in this chat: each background
command, subagent and schedule `/task:list` lists, and the answer being
written. `/task:stop tasks`, `agents` or `schedules` stops one kind. A
subagent already under way has its turn stopped too. It answers with
what stopped, and what didn't and why. [`/chat:stop`](../../../../../../../agent/docs/agents/chat/README.md#control-a-running-turn), or `/stop`, ends
the answer alone.

### Send a message every so often

[`/cron:create 10m check the build`](../../../../../../../tools/scheduler-tool/docs/agents/tools/scheduler/cron/README.md#schedule-a-prompt) sends the agent `check the build`
every ten minutes in this chat, and its answers come here. The interval
is minutes that divide an hour, hours that divide a day, or `1d` for each
midnight, in the agent's time zone. Its alias is `/loop`, and its JSON
takes any `cron`, with `recurring = false` to send it once.
[`/cron:list`](../../../../../../../tools/scheduler-tool/docs/agents/tools/scheduler/cron/README.md#list-or-delete-schedules) lists the schedules, and [`/cron:delete`](../../../../../../../tools/scheduler-tool/docs/agents/tools/scheduler/cron/README.md#list-or-delete-schedules) deletes one by its
`id`.

### Start a subagent

[`/task:agent {"prompt": "find three sources"}`](../../../../../../../tools/scheduler-tool/docs/agents/tools/scheduler/task/README.md#delegate-work) starts a subagent, as the
agent's `task__agent` does. It runs on the `model` the JSON names, a
model or an alias, else on this conversation's.
