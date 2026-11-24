# Developing

`lnk-scheduler-tool` is Link Scheduler: subagents and schedules for any
harness that reads who a message is from. [Link Scheduler](../README.md)
explains usage, and [scheduler decisions](decisions.md) explains the
durable-job and conversation-scope choices behind these mechanisms.

## Build and test

From the repository root, run `cargo build -p link-scheduler-tool`, then `cargo test -p link-scheduler-tool`.

## Its two programs

`serve --state <dir>` is the tool, an MCP server over stdio, and `run
--state <dir> --way-in <socket>` the runner. No number is the
program's own: [each comes from the agent's specification](../../../../../contract/docs/agents/tools/scheduler/more/contract.md).

### Their flags

Both take the caps (`--per-day`, `--subagents`, `--hops`, `--interval`,
`--late`), the connections a schedule's answer may go to
(`--connection`), and
the time zone its times are in (`--time-zone`, `local` or a zone's name,
which it sets as the process's `TZ` after checking the machine's zone
database has it). The runner also takes how long it waits on a firing
(`--timeout`), which Link reads from the agent's specification (`[tool.scheduler]
firing`).

### The runner as its agent

The runner, and the tool `serve` makes, act as its agent alone (`--as
<agent id>`), on a team or not, so a schedule's firings and their retries
keep one identity. Each of the runner's firings is
a bridge's call (`link_sandbox_bridge`), proved with the agent's key, read from
the machine's keyring (`--keyring <dir>`, else Link's,
`~/.lnk/safe/agents/credentials`) at each firing, since the key is made at each
start. An agent that isn't running has no key, and its firing waits to
be tried again.

### A teammate's bridge

A teammate's bridge takes a message from the runner only as this
agent's, and only into its conversation with it or a team's they
share.

## A call's conversation

A call says which conversation it's made in, and how many messages led
to it, in its `_meta` (`link.local/thread`, `link.local/hops`), as Link
Harness sends them, never in arguments the model writes. The owner's
command line alone says `link.local/all` on a `list`, for every
conversation's jobs (`lnk agent task list --all`).

### None named

A `schedule`, `schedules` or `unschedule` call that names no
conversation, from a harness that can't or from the owner's command
line, is made in the agent's main chat, `link:main` (`LINK_MAIN` of the
`link` connection). Every other call that needs a conversation is
refused without one.

### Its standing

A call also says the standing of the message its conversation is
answering (`link.local/standing`: `owner`, `member` or `stranger`),
its sender's at most what that message's firing carried. Every job
keeps it, and its firing carries it to the bridge, which tells the
harness, so a schedule, a subagent or a message to a teammate never
fires as more than the conversation that made it. A message a teammate
relays stands as a member's, and a schedule from the agent's specification as its
owner's.

### Its floor

A call that says no standing, from a harness that sends no `_meta`,
keeps the agent's floor (`--floor`): `owner` while only its owner
messages it, `member` once a chat is shared with it or it's on a team,
as Link works it out from the agent's specification and its teams. A harness's
standing never goes above the floor, and one that isn't a standing makes
nothing.

### Its tools

A harness that keeps a conversation's tools says them too
(`link.local/tools`, their names): `task`'s `tools` must be among them,
and left out, the subagent gets them all, so a narrowed conversation's
subagent is never wider. Without `link.local/tools`, a job has no list
(the subagent gets every tool, as its harness gives them), and `task`
refuses an explicit tools list.

### The job's list

A schedule a subagent makes keeps its conversation's list too, so it
fires with no more; another conversation's isn't kept, since a list
narrows the conversation it fires in for good. The job keeps the list
(`tools`), and its firing carries it to the
bridge, which sends it with the task (`Link-Tools`); said as anything
but a list of names, it's read as none.

### Its parent

A subagent's firing also names the conversation that started it
(`parent`, the thread its answer goes to). A harness that keeps its
work in a store of its own records the subagent as that conversation's
child from it before taking the task, as the
[harness protocol](../../../../../../../harnesses/portability/contract/docs/agents/harnesses/more/protocol/endpoint.md#parent)
says.

### Scoped to it

A call is scoped to its conversation: `list`, `read`, `cancel` and
`task`'s `after` see the jobs sent to it, the subagents it started
(`answer` its thread), and theirs, and the messages to teammates sent
from it, waiting, being fired or ended. `list` gives the soonest
100, and `cut`, how many more there are.

## Teams

An agent on a team is given its ID (`--agent-id`), its teams (`--team-id
<name>=<id>`) and their other members (`--teammate <team>/<name>=<agent
id>`), and the runner each teammate's way in (`--way-in-of <agent
id>=<socket>`).

### `send`

`send` is offered only with teammates, `to` one of their names (or
`<team>/<name>` where two teams' members share one) or a team's, a job
each (`kind` `send`, `to` the two agents' IDs). Its thread is the
teammate's `agent-<this agent's ID>` (or `team-<team ID>`), and its
`answer` the conversation it was sent from.

### Its firing and answer

The runner fires it at the teammate's way in, from `agent:<this agent's
ID>`, its answer `nowhere` there, and keeps what it's answered as a job
of its own (`kind` `reply`, from `agent:<teammate's ID>`, one hop on)
to that conversation. So delivering the answer, failed and tried again,
never sends the message twice.

### A refusal

A way in that reads a firing and won't take it, by its terms or a cap,
answers the bridge's error envelope with `"why": "refused"`
(`link_sandbox_binding::Why::Refused`), which `link_connection_contract::fire`
keeps. The runner ends such a firing at once, failed with the refusal,
and never tries it again. For a `send`, it keeps a `reply` job saying
the refusal, from the teammate, one hop on, to the conversation the
message was sent from, so the model learns why it stopped. A `reply`
refused in turn ends there and tells no one.

### The team relay

On a machine that carries no message itself, a team's message to a
teammate on none of its ways in goes through the team relay: the
account that hosts the team, at `--team-relay <url>`, for each team
given its owner (`--team-owner <team>=<account>`). It needs
`--agent-id`, and isn't taken with `--driver address`, whose machine
carries messages already. The wire is `link_agent_place::relay`.
Link's agent module gives these flags from a hosting account's
`agent-team list` (`link_scheduler_tool_contract::relay`).

### A message out

`Relayed` holds the runner's driver. A `send` job to a teammate that
driver doesn't reach, on a team's conversation (`team-<ID>`) the relay
carries, is posted as a `TeamSend` for that teammate (`to`), with its
hops and text, to `/v1/teams/<owner>/<team>/messages`. It's done once
the relay took it, and a refusal is a failure, tried again as any
firing is. A message on the agent's own conversation with a teammate
(`agent-<ID>`) never goes through the relay.

### Messages in

At each look, and at each wake of a runner that isn't resident, the
runner asks its driver for what waits elsewhere (`Driver::arrivals`).
`Relayed` pulls each team's mail after the last message it had
(`?after=<seq>`, kept by team in `team-relay.json` in the state, naming
its schema) and puts each message as a job of its own. It's a `reply` from
`agent:<the sender's ID>` on the team's conversation, with its hops,
answered nowhere, and fires through the agent's way in as any job does.

### Its cursor

The cursor moves past a message once its job is kept, so a failure
pulls it again. A look takes at most four pages of 64, and none while
1,000 jobs wait. On a runner for good, a message comes in within the
minute it looks at most.

## Where the jobs are kept

The jobs are kept behind a seam, `JobStore` (`src/store.rs`): put, get,
list, count, move between waiting and being fired, remove, when the
soonest is due, and the day's count, so a machine whose agent keeps its
state off it gives another. `store::Memory` keeps them in memory for
tests.

### The locks

The rest of the store (`runner.lock` and `post.lock`) stays in the
state folder on the machine. Each lock is opened in place, never
through a link put at its name (`O_NOFOLLOW`), so whoever else writes
the state folder can't point the runner or a post at a file elsewhere.
The runner holds its shared lock until its firings end. The multi-agent
test observes that lock released before checking that a removed agent's
jobs stay queued while another agent's jobs continue to fire.

### The file store

The store uses the shared file driver (`store::Stored`,
`link_store_contract::store::file`). `schedule/jobs/<id>.json` holds
each job, written aside and moved in place, so the tool and the runner
never see half of one, and `schedule/day.json` the day's count, the day
in its time zone.

### `firing/`

`schedule/firing/<id>.json` is a job the runner took to fire, moved
there from `schedule/jobs/` (a cancel meanwhile wins the move) and
back with its next time or its next try once the firing ends, or
removed. A subagent is at work while its task is in either folder. At
its start, a runner alone moves what's in `firing/` back, each counted
as a try.

### `ended/`

A delegation (a subagent, a message to a child or a teammate) that ends
for good is kept in `schedule/ended/<id>.json` with its `outcome`
(`{"at", "ended": "done" | "failed" | "cancelled", "answer" | "why"}`),
the store contract's `ended` place, never due. Ended jobs older than a
week, and past the 256 most recent, are forgotten. These numbers and the
tool's other limits (the longest message, the most jobs waiting and
listed, a firing's retries and lateness, the runner's interval, and the
relay's pulls) are in `src/limits.toml`, read once into `Limits`.

### `parent` and `after`

A job keeps its `parent` (the task that started it, from the conversation
it was started in) and `after` (the jobs it waits on, each its own
conversation's). The runner fires a job whose `after` all ended done,
and cancels one whose `after` failed, was cancelled or is gone.

### Cascading

A cancel or a failure cancels the jobs whose `parent` or `after` names
it, under the jobs' lock; ending one wakes the runner for those waiting
on it.

### What it counts

The 1,000 jobs that may wait are counted by their files in `schedule/jobs/`,
through the store contract. Without a retained host, subagents are
counted by the waiting and firing jobs’ kinds; with one, retained child
state also counts until its result is delivered or its record is
archived or deleted. A move
carries each job and the day's count through that same contract.

### Portable Definitions

The runner accepts `--definitions <file>`, a JSON object naming its
schema whose `schedules` are the canonical portable schedules published
from the agent specification after native handoff.
The resident, calendar, addressed and shared runners project them into
the existing job store. The MCP tool does not activate native imports.

### Activation Records

Each definition has a stable identity and revision. Projection records
pending and ready activation under the scheduler's definitions lock,
each record naming its schema.
Dispatch refuses a pending or corrupt activation. Repeated projection
keeps completed or cancelled jobs completed or cancelled. A changed
definition requires a higher revision. Removing a definition cancels only
its projected jobs and preserves jobs created through the scheduler tool.

### Time and Occurrences

Cron expressions use each definition's explicit IANA time zone, including
clock changes, independently of the runner's default zone. Intervals use
an anchor in Unix seconds. A firing records its scheduled instant and
definition revision separately from its next retry time. Delivery keeps
that occurrence identity, while each later recurrence receives another.
The existing late-run policy still determines whether a missed firing
runs once or is skipped.

## Through the store bridge

`store::Bridge`, `--store-bridge <socket>` on `serve` and `run`: on a
machine that runs the agent only while it answers, its jobs are kept
off it, by the machine's store bridge and the driver it runs, so a
sleeping agent's jobs outlive its machine and whichever machine it's
bound to next reads them. It needs `--as`.

### Its calls

Each call is one of the store contract's job operations, which its [store drivers](../../../../../../../stores/contract/docs/agents/more/stores/protocol.md#store-drivers-lnk-name) serve: `job_put`, `job_get`, `job_list`, `job_count`, `job_shift`, `job_remove`, `job_next`, `job_day`, `job_set_day` and `job_mark`,
proved as the agent `--as` names with its key from the keyring
(`--keyring`, else Link's), read at each call, as each firing is.

### A job in the store

A job is its JSON, kept whole, its place and when it's due, so the
store answers when the soonest is due without reading one, and moves a
job between waiting and being fired whole, of two moves of one job one
made, on any machines. The mark the runner watches is the store's,
moved by each change to the waiting jobs.

### A call that fails

A call that fails is said on stderr and read as none (no job, none
listed, a move not made), as an unreadable file is in the folder. When
the jobs are next due, which the machine is told, is an error instead,
so a post never says none for a store it couldn't read.

### The next machine

`--state` keeps only the machine's locks, so the first runner on the
next machine, alone on its `runner.lock`, moves back what the last was
firing when the agent slept.

### `tests/scheduler/bridge.rs`

It runs the store bridge against
`link_store_contract::store::serve_bridge` over `lnk-sqlite-store`, built in
the workspace: the jobs firing as in the folder, a second machine reading a
sleeping agent's, each agent's its own, and the tool and a wake on the
address driver through the program.

## The runner

The runner looks at every job when the soonest is due, a minute at
most, and else only at the store's change mark, once a second.

### Its drivers

It fires through a driver: `WayIn`, the agent's bridge's way in (a Unix
socket, one JSON line each way, `link_connection_contract::Firing` and
`Fired`), or `InProcess`, a function, for tests and a harness running
it in its own process. The same tests run on these two and `Cron`.

### Drivers that aren't resident

`Cron` holds another driver and isn't resident (`Driver::resident`):
`run --driver cron` fires what's due, waits for those firings to end,
and stops (`wake`). `Addressed` holds another too and isn't resident
either: its wake then tells the machine when the jobs are next due
(`Driver::woken`).

### A runner for good

A runner for good (the service driver, alone or `run --agents`) ends
once a SIGTERM asks it to, as its bridge stopping it does, or once the
process that started it is gone, either looked at each second. Either
way it ends in order, the firings it waits on ended, as when it's taken
off `run --agents`' list.

### None outlives its bridge

So none outlives its agent's bridge, however that ends (stopped,
killed, its terminal closed), and the next bridge's runner is the only
one. A woken one (`cron`, `address`) fires what's due and stops anyway
(`tests/scheduler/many.rs`).

### Runners beside each other

Each holds a lock on `runner.lock` in the state while it works. The
first, alone, moves what's in `firing/` back; one started while another
still waits on a firing, as a minute's wake may, leaves that firing
alone.

## One runner for many agents

`run --agents <file> --time-zone <zone>` fires for every agent `<file>`
lists (a JSON list, each agent's flags as `run` takes them alone), each
through its own way in, all in `--time-zone`, since the zone is the
process's. Link's bridge for every agent of a machine runs one a zone
(`tests/scheduler/many.rs`).

### Its list changing

It reads the file again when it changes: an agent added starts, and
one gone, or listed with other flags, stops once its firings have
ended, holding `runner.lock` till then, so a runner after it never
fires one of them again.

### What it doesn't run

One in another zone, or with `--driver cron`, isn't run, and its log
says why. With none listed and none left firing, it ends.

## The address driver

For a machine that runs an agent only while it answers: `--driver
address --socket <socket>`, the socket the machine gives (on a machine
that packs agents, its `scheduler.sock`). `tests/scheduler/address.rs` runs it
through the program, the machine a stand-in on a Unix socket checking
each proof.

### Its post

Whenever the jobs change, the scheduler posts when they're next due
there (`Address::post`): a bridge's call (`link_sandbox_bridge::call_at`)
proved as the agent `--as` names, with its key from the keyring
(`--keyring`, else Link's), as each firing is. Its action is `{"next":
<seconds since 1970>}`, or `{"next": null}` with none waiting,
answered `{}` or `{"error": "<why>"}`.

### When `serve` posts

`serve` posts after each call except the read-only `list`, `read` and
`schedules`; a post that fails makes the
call an error that says the job is kept. Posts take turns on
`post.lock`, each reading the jobs once it's its turn, so the machine's
last word is the jobs' last state. A machine refusing a post (one
proved as another agent, say) fails the wake.

### One wake

`run --driver address` is one wake: whatever woke the agent at that
time runs it once, it fires what's due through the way in, waits for
those firings to end, posts the next time, and returns. Nothing of it
runs between firings, and `run --agents` and `cron install` refuse it.

### A teammate elsewhere

A message to a teammate on none of the machine's ways in
(`--way-in-of`) goes to the machine the same way, proved as this agent,
its action `{"firing": {"to": <the teammate's agent ID>, "firing": <the
Firing>}}`, answered `{}` once the machine has taken it to carry
wherever that agent is (`Address::send`).

### Its answer

Nothing waits for the teammate's answer, which comes back as a message
of its own: its thread is the teammate's, as a teammate's answer is on
a machine that holds both.

## The cron driver

It's woken once a minute by the machine's calendar. Link's harness
module runs `cron install`, labelled by the agent's ID, when the
agent's specification says `[tool.scheduler] driver = "cron"`, and the bridge
then runs no runner of its own.

### `cron install` and `cron uninstall`

`cron install --label <name> <run's flags>` writes a line of the user's
crontab, ended by `# lnk-scheduler-tool <name>`, or on a Mac a
LaunchAgent (`~/Library/LaunchAgents`) whose `StartCalendarInterval` is
every minute, loaded with `launchctl` (asked nothing with
`LNK_SERVICES=off`). Installing again replaces it, and `cron uninstall
--label <name>` takes it away.

### Its output and paths

Its output is added to `cron.log` in the state, and its paths must be
absolute, since the calendar starts it from your home.

### Missed minutes

The calendar only wakes it: each job fires by its own time, in the
scheduler's zone. A Mac that slept is woken once for the minutes it
missed; cron starts nothing for them, and the next minute fires the
missed jobs as `late` says.

### `LNK_CRONTAB`

`LNK_CRONTAB` names a program to stand in for `crontab`, on any
machine, as the tests do (`tests/scheduler/cron.rs`).
