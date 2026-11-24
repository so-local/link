# Scheduler Contract

`link-scheduler-tool-contract` turns an agent's specification (`[tool.scheduler]`)
and its teams into the flags Link Scheduler takes, so its numbers are
always the specification's, never the program's nor the machine's. The agent
module starts the agent's scheduler with them, and any machine running
the scheduler for an agent passes them too.

## Its flags

What `lnk-scheduler-tool serve` and `run` are given, each read from the
specification's `[tool.scheduler]` (`spec.tool("scheduler")`).

### `caps`

`--per-day`, `--subagents`, `--hops`, `--interval`, `--late` (`run` or
`skip`) and `--time-zone`, from the specification's `per_day`, `subagents`,
`hops`, `interval`, `late` and `zone`. Missing `per_day`, `subagents`, `hops`, or `zone` is refused by name. An omitted `interval` sets no minimum, and an omitted `late` uses `run`. Link’s own specification sets all six to 100, 5, 8, `5m`, `run`, and `local`.

### `teams`

`--agent-id <id>`, then for each team the agent is on `--team-id
<name>=<id>` and each other member as `--teammate
<team>/<name>=<agent id>`, whom `send` may reach. An agent on no team
gets none of them.

### `floor_of`

`--floor owner` or `--floor member`, the agent's floor: the least
standing anyone reaches it with. It's `member` once the specification shares a
chat with the agent (`[access] message`) or it's on a team, and `owner`
otherwise. A call to the scheduler that says no standing makes jobs that
keep it, and a call's own standing never goes above it. `serve_flags`
and `run_flags` include it, and the agent's own serve line does too.

### `relay`

`--team-relay <url>`, then `--team-owner <team>=<account>` for each team
the agent is on that the relay carries (`RelayedTeam`, from a hosting
account's `agent-team list`). The runner takes one relay: the first, by
the order of the agent's teams, so a team another relay carries is left
out. An agent on no relayed team gets none of them.

### `timeout`

`--timeout`, the specification's `[tool.scheduler] firing`, how long the runner
waits on a firing (30 minutes in Link's specification), a duration read as the
specification reads it. It's the runner's alone, never `serve`'s, and none when
the specification sets none.

### `serve_flags` and `run_flags`

`serve_flags` is the caps, the teams, and the floor. `run_flags` is
those, then `--timeout`, which `run` requires: a specification with no
`[tool.scheduler] firing` is refused for it, while `serve` needs none.

### Typed lines

Each of these returns the typed `Flags` the tool's `actions.toml`
declares for its group, never strings: a caller joins two with
`with`, and writes a line once, with `serve(program)` or
`run(program)`, through the generated `argv()`. `Runner` is a program
and its `Flags`, the line an agent runs its jobs with; its `argv()` is
that line. `run`'s `--agents` file (`listed::Listed`) is a JSON object
naming its schema whose `agents` are the same `Flags`, one runner per
time zone.

## What never asks you

`allowed` names the calls of its tool that never ask you: `list`,
`read` and `schedules`, which only read, and, unless the specification says
`[tool.scheduler] ask = true`, `task`, `message`, `schedule`, `send`,
`cancel` and `unschedule`. A recurring schedule asks you whatever
`allowed` says, unless the specification lets it through.

### `ask_if`

`ask_if` names the calls that ask by an argument, as the sandbox takes
them (`--ask-if schedule.recurring`): each argument the tool declares
that asks, but none of the `recurring` argument when the specification says
`[tool.scheduler] recurring = "allow"`.

### `sandbox_flags`

`sandbox_flags` is what `lnk
sandbox tool add` is given before the tool's command: an `--add` for
each call `allowed` names, then, while any call asks by an argument,
`--ask-if-hint` and an `--ask-if` for each. The hint, from
`asking.toml`, names the setting that lets a recurring schedule
through, so a refusal with no one to answer points there rather than at
`--add`, which an argument that asks doesn't heed.

### `tool_of`

The tool an agent's scheduler is served as on its proxy (`lnk sandbox
tool add`) is `scheduler-<agent id>`.

## Portable Timing

The contract also validates canonical schedule timing and calculates the
next occurrence. `cron::Cron::in_zone` uses an explicit IANA time zone.
`timing::next` preserves an interval's anchor and calculates its next
instant independently of the runner's default zone. Import and execution
use these same rules, so an unsupported zone or malformed cron is refused
before the managed native source is cleared.
