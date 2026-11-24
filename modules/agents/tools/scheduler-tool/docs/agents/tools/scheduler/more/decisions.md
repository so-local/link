# Scheduler Decisions

Why delegated tasks use the scheduler’s durable jobs, conversation scope,
and cancellation rules.

## Why is a delegated task a scheduler job?

Because Link Scheduler already keeps jobs durably, fires them through
an agent's way in, counts them against its caps and carries them
through a move. A subagent, a message to a teammate and the steps after
them are each a job, and one that ended is kept in [the store contract's
`ended` place](developing.md#where-the-jobs-are-kept) with its outcome.
A separate task service would need its own store, caps and moves, and a
team needs no other.

## Who says what a task waits on?

The scheduler, outside the model. A task's coordinator is the
conversation the call came from, never a field the model writes, and
`after` takes only that conversation's own delegations. So a model
can't attach its work to another conversation's, wait on what it never
started, or slip past the hop, daily and subagent caps a team's members
already share.

## What does cancelling a task cancel?

Its children and what waits on it, and in turn theirs, under the jobs'
lock. A step whose input failed or was cancelled is cancelled rather
than run on what never came, so a coordinator never pays for work whose
plan already broke. Without a retained host, a running subagent finishes
its turn and its answer is discarded. A retained subagent already
admitted by its host is left running: direct cancellation refuses it,
and a cascade leaves it alone.

## Why does a recurring schedule ask first?

A recurring schedule acts again and again with no one watching, so
`cron__create` asks you unless it says `recurring = false`; a one-off
reminder doesn't. The owner sees every conversation's schedules with
`lnk agent task list --all`.

## How does an owner let recurring schedules through?

With the agent's specification, `[tool.scheduler] recurring = "allow"`. An
agent with no one to answer, in the background or on a box, would have
every recurring schedule refused, and `--allow` can't help: an argument
that asks (`--ask-if`) asks even of a call `--allow` names. The setting
takes the `--ask-if` away for that agent alone, and leaves `ask` and
every other ask as they are.

## Why is the recurring setting the specification's?

The specification is a control file the agent never writes, so a model can't let
its own schedules through, and its default, `ask`, keeps the reason
above. A refusal names the setting,
carried to the sandbox as the hint of what lets the call through
(`--ask-if-hint`), so the sandbox stays unaware of the scheduler.

## Why is there a shortest interval?

An every-minute cron would spend a day's messages in under two hours,
so a recurring schedule repeats no more often than the specification's
`interval`, five minutes in Link's. A subagent's work is outside the
day's budget, which is for what schedules send.

## What happens across a daylight saving change?

As Vixie cron does: a time skipped forward runs once, at the change, and
a time that repeats runs once, so no firing is lost or doubled.
