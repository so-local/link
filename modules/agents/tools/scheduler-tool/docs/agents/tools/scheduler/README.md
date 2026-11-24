# Scheduler

> Scheduler Sends Subagents and Timed Messages

**Your agent can start subagents, schedule messages to itself, and
message the rest of its team.** Ask your agent for a briefing at six,
a check every hour, or three searches at once.

<!-- quickstart -->

## Add Scheduler to Your Agent

Start your agent and enable the scheduler tool, then review any requested permissions. Check what Scheduler [requires and what the sandbox grants](guide.md).

```sh
lnk agent start
lnk agent tool add scheduler
```

## Ask Your Agent to Schedule or Delegate

Ask your running agent to schedule a message or delegate a task. Learn [what Scheduler can do and how to configure access](guide.md).

```sh
lnk agent chat
```

## Run Scheduler Actions from a Terminal

Delegate or run work with `lnk agent task`, the shared command namespace for Run and Scheduler. [Task Actions](task/README.md) covers request input and choosing the provider when identifiers overlap. Schedule a prompt, and list or delete schedules, with `lnk agent cron`. [Cron Actions](cron/README.md) covers each of those actions.

## Learn How Scheduler Works

Trace how work survives a turn or a machine change, and how caps apply, in
[Developing](more/developing.md). Developing explains the tool process, durable
runner, job storage, and authenticated call boundaries. [Scheduler Internals](more/README.md)
connects the launch contract and the design decisions.

<!-- docs-tree:start -->

## Docs

### Pages

- [Scheduler Guide](guide.md): Enable Scheduler to delegate tasks, send team messages, and arrange future turns for your agent
- [Task Actions](task/README.md): Use `lnk agent task` to delegate work or run a command through an agent’s enabled providers
- [Cron Actions](cron/README.md): Use `lnk agent cron` to schedule a prompt for an agent from your terminal, and to list or delete its schedules

### More

- [Scheduler Internals](more/README.md): Inspect the scheduler’s launch contract, durable runner, and delegation boundaries

<!-- docs-tree:end -->
