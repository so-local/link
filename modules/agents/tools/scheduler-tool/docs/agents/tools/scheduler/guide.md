# Scheduler Guide

Enable Scheduler to delegate tasks, send team messages, and arrange future turns for your agent. Set its limits before creating recurring work that may spend model tokens.

```sh
lnk up
lnk agent start
lnk agent tool add scheduler
```

## What it needs

You need the agents modules, which [`lnk up`](../../../../../../../../docs/more/modules/install.md#pick-your-modules) installs and which
bring Link Scheduler, and a harness that reads who a message is from.
Link Harness, OpenClaw, Hermes Agent and DeepSeek Harness do. A new agent
has the scheduler off: [`lnk agent tool add scheduler`](../../../../../../agent/docs/agents/README.md#add-tools-and-skills) turns it on, and
each start then gives the agent its scheduler.

## What your agent can do

Your agent can start a subagent, schedule a prompt for itself, and list
or cancel either. Its actions take the shapes models are trained to
call, so a model uses them as it already knows how. Subagents are
`task__agent`, `task__send`, `task__list`, `task__read`, and
`task__stop`, as an Agent tool and its task controls are. Schedules are
`cron__create`, `cron__delete`, and `cron__list`, as CronCreate, CronDelete
and CronList are. The scheduler provider calls described below supply
those actions, and Run shares the `task` namespace for command
execution.

### Delegate a Task

`task__agent` starts a subagent, a new conversation of its own that begins now
with the `prompt` it's given, and a short `description` that
`task__list` shows. The subagent has the same tools, or fewer if
`task__agent` is given a list of names, and it sees nothing else of the
conversation it came from. It runs on the model `task__agent` names, a model or
an alias of the specification's `[model.aliases]`, else on the one its
conversation runs on. Its answer comes back to that conversation, so
your agent can start three, say so, and answer when they're in.

### Steps after others

`task__agent` with `after`, a list of IDs of this conversation's subagents or
messages to teammates, starts once each of them answered, so a
coordinator can plan design, then build, then review. One that failed
or was cancelled cancels what waits on it, and what waits on that. A
subagent's own subagents are its children: cancelling it cancels them.
What each ended with is kept for a week, the newest 256 at most, its
answer up to 16 KiB, for `task__list` and `task__read`, through a
restart and a move of the agent.

### Retained Children and Messages

With the Link Harness, a child has a stable task ID before it starts. `task__send` sends a `message` to an existing retained child by that ID; it never creates a replacement or releases a pause or owner hold. `task__list` includes retained child state, and `task__read` returns the complete exact job for proposal review. A paused child's original task, progress and return destination remain available for resumption. A final result retains its original destination across recovery. Confirmed delivery releases the retained scheduler job without executing the task again.

### Stop a Subagent

A subagent already at work isn't stopped mid-turn. Without a retained
host, cancelling it ends its job, its turn runs to its end, and its
answer still comes back. Link Harness admits a subagent as retained
work, and cancelling one it admitted is refused, reporting that its
answer is coming. A subagent that already answered can be cancelled
too, which takes the schedules and subagents it left waiting with it.
Once cancelled, a subagent starts nothing more.

### List or Cancel Work

`task__list` shows what’s scheduled in this conversation, the subagents it
started that haven't answered, and how those that ended did, the soonest
100 and how many more. `task__stop` cancels one by its `id`, with its
children and what waits on it. Use `kind = "schedule"` when Run is also
enabled to select scheduler jobs. A conversation never sees or cancels
another's.

#### Schedules alone

`cron__list` and `cron__delete` see schedules alone, as CronList and
CronDelete do. They are the scheduler's `schedules` and `unschedule`
calls, which list no subagent or message and refuse to delete one. A
deleted schedule is gone, with what waited on it: unlike a cancelled
subagent, which `task__list` still shows as ended, no list shows it
again.

### See Every Conversation's Work

You see every conversation's schedules and subagents from your own
terminal, not only one chat's.

```sh
lnk agent task list --all
```

Only your command line says `--all`: a conversation's own list never
shows another's.

### Asked first

Your agent does these without asking you, but for a recurring schedule,
which asks you first, as [A Recurring Schedule Asks
First](#a-recurring-schedule-asks-first) says. Set `[tool.scheduler] ask =
true` in its specification and each waits for you to allow it, as any tool's call
you haven't allowed does, in [`lnk sandbox question list`](../../../../../../../security/sandbox/docs/security/sandbox/tools/questions/README.md#answer-what-a-sandbox-asks). A message from a schedule
or a subagent is never taken for yours, and your agent is told who's
speaking. A recurring schedule starts without asking too once its specification
says `[tool.scheduler] recurring = "allow"`, which only you can set.

## Schedule a Prompt

`cron__create` sends your agent a `prompt` by a standard five-field
`cron`, as `0 6 * * 1-5` for six on weekdays. It recurs on every match
until `cron__delete` deletes it, or, with `recurring = false`, fires
once at the next match and is gone, so a reminder pins its minute, hour,
day of the month and month (`30 14 28 2 *`). Each time it fires, your
agent gets the prompt in a conversation of its own, which sees nothing
else of the one that made it, and its answer comes to you on this chat.

### A Check That Speaks Only With News

Ask your agent to "check the forecast every morning at 7 and tell me
only if it will rain". It schedules a check whose `answer` is
`nowhere`, with a prompt that messages you with `agent__send` only when
there's news, as `cron__create`'s description teaches it. A dry morning
then sends nothing, and a wet one reaches you where `agent__send` sends,
your chat on a connected service among them.

### Its Time Zone

A cron is read on the agent's clock, in its time zone, this machine's
unless its specification's `[tool.scheduler] zone` names one. The agent is told
its zone and its offset from UTC, and sees each schedule's next time on
that clock, with its offset (`2026-10-06T18:00:00+02:00`).

### Daylight Saving

A schedule at a fixed time of day keeps to Vixie cron's rule when the
clocks change. A time the clocks skip, such as 02:30 the night they go
forward, fires once, at the moment they jump. A time they repeat, such
as 01:30 the night they go back, fires the first time only. A schedule
with `*` in its minute or hour, such as `*/15 * * * *`, fires by the
clock as it reads, so the repeated hour runs twice.

### How Often It Repeats

A recurring schedule fires no more often than the specification's
`[tool.scheduler] interval`, five minutes in Link's. One sent once
repeats nothing, so it's never held to it. A `cron` firing
more often, such as `* * * * *`, is refused when it's made, and nothing
is scheduled. In `cron`, a day field starting with `*`, such as `*/2`,
narrows the other day field rather than adding to it, as in Vixie cron.

### Schedules in Other Conversations

A schedule a subagent makes answers the conversation that started the
subagent, as the subagent's own answer does, and goes with the subagent
when it's cancelled. A teammate's conversation, or a schedule's own,
has no chat to answer in, so a schedule made there names its `answer`,
and one that doesn't is refused when it's made.

### Schedules From Harnesses Without Conversations

Some harnesses, such as Hermes, call Link's scheduler without naming the
conversation the call came from. A schedule made that way belongs to the
agent's main chat, `link:main`. It fires there, and its answer comes to
you there, held for your next [`lnk agent chat`](../../../../../../agent/docs/agents/chat/README.md#manage-history). `cron__list` and
`cron__delete` from such a harness see and delete the main chat's
schedules, so Link's scheduler holds every schedule the agent makes.

### A Recurring Schedule Asks First

A recurring schedule runs on its own, again and again, so `cron__create`
waits for you to allow it unless it says `recurring = false`, as any
tool's call you haven't allowed does. A one-off reminder never asks.
The scheduler declares its `recurring` argument as one that asks, and
Link gives the sandbox it (`--ask-if schedule.recurring`), so the model
calls the shape it's trained on and the asking stays the sandbox's.

### Let Recurring Schedules Through

An agent that runs with no one to answer, such as one on a box, has its
recurring schedules refused at once. Its refusal names the setting that
lets them through, since [`lnk sandbox tool add --add`](../../../../../../../security/sandbox/docs/security/sandbox/reach.md#call-a-tool-that-runs-outside) can't. To let
the agent make recurring schedules without asking you, set it in the
agent's specification:

```toml
schema = 1
[tool.scheduler]
recurring = "allow"
```

The same goes as a flag of [`lnk agent start`](../../../../../../agent/docs/agents/README.md#create-an-agent), `--tool.scheduler.recurring
allow`. Link then gives the sandbox no `--ask-if` for that agent. The
setting is the specification's, which the agent can't write, so only you let its
schedules through. The ready-made coach and personal agents set it in the
specifications their archives hold, since they make their own check-ins and
morning briefs. `lnk agent start
--tool.scheduler.recurring ask` makes theirs ask again. `[tool.scheduler] ask = true` still asks of every
call that writes, a recurring schedule's among them.

### Your Own Schedules Don't Ask

Only the model's calls ask. A schedule you make yourself from your own
command line, `lnk agent cron create`, is yours already, so it doesn't
ask: that command runs outside every sandbox and tells the namespace so
(`lnk-sandbox mcp-namespace --by owner`), which nothing inside one can.
A whole call that asks, such as `cancel` under `[tool.scheduler] ask =
true`, still asks. Your own command line needs no `recurring = "allow"`,
which is for the model's schedules.

### Occurrence Controls

Each registered firing runs in its own occurrence conversation while retaining the original answer destination. Delivery retries preserve that occurrence's identity. Definition-only pause blocks later starts and lets an admitted occurrence finish; selecting its subtree also addresses retained occurrences. Changing a definition revision invalidates old occurrence approvals. A response from one occurrence never authorizes a later firing.

## Choose where a scheduled answer arrives

Ask for a joke on Telegram in three minutes, or a reminder on Discord.
`cron__create`'s `answer` can name a connected service, such as
`telegram`, `discord`, `slack` or `signal`, or `cli`. The owner receives
the answer in their direct chat there; a CLI answer waits for an active
or newly opened CLI chat. `here` keeps the current chat and `nowhere`
keeps the result private. The agent’s `agent__send` action sends
immediately when no timer is needed.

### A Service Not Connected

A service the agent isn't connected to is refused when the schedule is
made, naming where an answer may go, and the model is offered only those
names.

## Message its team

On a [team](../../../../../../agent/docs/agents/team/README.md#make-a-team), your agent has
the team messaging action, `agent__send`, to message a teammate by its name, or
every member at once by the team's name. The message lands in the
teammate's conversation with your agent (or the team's), marked as
from your agent, and the teammate's answer comes back to the
conversation it was sent from, as a message from it, never from you.
So the member you text can hand out parts of a task, say it has, and
answer you once their answers are in.

```sh
lnk agent team create research --with scout --with writer --with checker
```

### Who it may send to

It may send only to the members of its teams, as their files say: each
teammate's bridge takes a message from another agent only when the two
share a team. Every message and every answer is a hop, so two members
answering each other stop at the cap on hops, and `send` says so.

### When a teammate refuses

A teammate may still refuse a message, past its own cap a day say, or
once its chat has had as many agents' messages as its hops since a
person last spoke there. Such a refusal is final. The message is never
tried again, and your agent is told why in the conversation it sent
from, as the teammate's answer would come.

## Turn it off

The scheduler is off until you add it. [`lnk agent tool remove scheduler`](../../../../../../agent/docs/agents/README.md#add-tools-and-skills)
turns it off again, or set it in your agent's
[specification](../../../../../../agent/docs/config/specs/agent.md), which you give with
[`lnk agent start --spec <file>`](../../../../../../agent/docs/agents/README.md#create-an-agent).

```toml
schema = 1
[tool.scheduler]
enabled = false
```

## How it runs

Your agent's bridge fires each job when it's due. The jobs are kept in
the agent's own folder (`~/Link/Agents/<agent>/agent/schedule`), and
the bridge is the background service that carries its connections and
starts again with your computer.

### One bridge for every agent

Where `~/.lnk/safe/agents/policy.toml` says `shared_bridge = true`, one bridge carries
every agent, and one runner fires every agent's jobs in each time zone.

### `driver = "cron"`

With `[tool.scheduler] driver = "cron"` in your agent's specification, your
computer's calendar wakes it instead, once a minute (launchd on a Mac,
cron elsewhere), so nothing of it runs between firings: `lnk agent
start` puts the entry in place and `lnk agent stop` takes it away. A
start in a terminal (`--foreground`) does too, as on a machine with no
service manager, and takes it away when it ends.

### `driver = "address"`

On a machine that runs your agent only while it answers, `[tool.scheduler]
driver = "address"`: the scheduler tells the machine when your agent's
jobs are next due, each time they change, and the machine wakes your
agent then to fire them. Nothing of the scheduler runs between.

### A firing that fails

A job is kept until its firing ends, and a restart loses nothing. A
message cut off by a restart may come twice. A firing that fails, say
because the bridge isn't up yet or the model provider is unreachable,
is tried again a minute later, then two, four, eight and sixteen
minutes later. After that it's given up, and the bridge's log says so.
A firing the agent's bridge refuses, by its terms or a cap, is never
tried again, since trying changes nothing.
A firing that takes longer than the agent's `firing` limit, 30 minutes
by default, is given up too, and never sent again.

### A computer that was asleep

A computer that was asleep through a firing fires it once on waking, or
skips it, as `[tool.scheduler] late` says. A recurring job never fires
once for every time it missed. `late = "skip"` skips only schedules: a
subagent waiting on others, a message to a teammate, and its answer
still fire. A conversation's compaction missed this
way is always skipped, since the prompt cache it was timed for is gone.

### Where the answer goes

The answer goes where the job says: to the chat, the owner's selected
connection, or the conversation a
subagent came from, or nowhere. The chat is the one you talk to your
agent in, on Telegram, Slack, Discord or Signal, or a group where you
talked to it, unless your agent's specification says it never answers in groups.
On Signal the answer goes out while the agent's bridge is running, as
it does whenever the agent is started.

### A quiet conversation's compaction

The scheduler also keeps one job for each conversation that the agent
itself sets, never the model. It wakes the agent once a conversation
has been quiet for its connection's `compact_after`, 55 minutes in
Link's for a chat on Telegram, Slack, Discord or Signal, so [the conversation compacts](../../../../../../harnesses/link/docs/agents/link/conversations.md#after-a-quiet-hour).
Each turn moves it later. Your agent never lists, reads or cancels it,
and it counts toward none of the day's messages.

### How the harness sets it

The harness sets the job with `compact`, a call the scheduler declares
and always allows, even with `[tool.scheduler] ask`. It reaches the call
through its `cron` namespace, which offers it to no model.

## Its limits and settings

Link's limits are 100 messages a day, 5 subagents at work at once,
chains 8 deep, as when a subagent starts a subagent, and recurring
schedules five minutes apart at least. They are keys of
your agent's specification under `[tool.scheduler]`, and
[Agent Specification](../../../../../../agent/docs/config/specs/agent.md) lists them all.

### The day's cap

The day's cap counts schedules' messages, and messages between
teammates, in the day of the agent's time zone. Past it, the tool says
so and none of those fire: a job due once waits for the next day, and a
recurring one for its first time then. The cap is what bounds how many
messages agents send each other. A subagent's work and a conversation's
compaction are outside it, so a busy schedule never stops delegation;
how many subagents run at once and how deep a chain goes bound them.

## More about it

Developing it, how its runner fires and where its jobs are kept:
[Developing](more/developing.md), and the flags an agent's specification gives it: [Scheduler
Contract](../../../../contract/docs/agents/tools/scheduler/more/contract.md).
