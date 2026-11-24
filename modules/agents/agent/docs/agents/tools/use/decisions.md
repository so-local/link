# Decisions

Why scheduled work and subagents use bounded tools and retained message delivery.

## Why doesn't the scheduler ask before it starts a subagent?

Because every subagent would otherwise wait on you, and the caps already
bound them. Whether `task`, `schedule` and `cancel` ask first is an
opinion, so a key of the agent's specification (`[tool.scheduler] ask`), and
Link's is not to ask. With it on, the three aren't allowed when the tool
is defined, so each asks as any tool's call you haven't allowed does.

## Can an agent turn its scheduler off?

Yes, in its specification (`[tool.scheduler] enabled = false`): at the next start
the scheduler isn't defined, nothing runs its jobs, and a harness that
reads who a message is from has none. Without the key, every such
harness has one, since a subagent and a schedule are what people ask an
agent for. There's no command for it: the key is the one way, as for
every other opinion in the specification.

## Why can a specification turn a browser or desktop on?

So an agent can come with its screen. A browser or desktop is defined
for each machine, its program's path with it, so an archive can't carry
the definition. It can carry the agent's specification, and `[tool.browser]
enabled = true` there makes its first start define one wherever it
lands, installing the module as `lnk agent tool add browser` does.
`false` takes it away at the next start. Link's own specification says neither,
so an agent whose specification is silent keeps what `lnk agent tool add` and
`remove` gave it, and those two commands write the key as they change
the tool.

## What wakes the scheduler?

The agent's bridge, by Link's specification, since it already runs for the
connections and restarts with the machine: the runner sleeps beside it
until a job is due. A machine where a resident process costs too much
can say `[tool.scheduler] driver = "cron"` instead: the machine's
calendar starts the runner once a minute, and nothing of it runs
between.

### A key, not a guess

It's a key, as every opinion is, rather than something Link guesses from
the machine. A machine that runs an agent only while it answers says
`address`: the scheduler tells it when the jobs are next due, and it
wakes the agent then, so even a calendar's minute isn't spent on an
agent with nothing due.

### The bridge still runs

Whichever wakes the scheduler, the bridge still runs, since a firing
reaches the agent through its way in. Where one bridge carries every
agent, one runner fires every agent's jobs in a time zone, each through
that agent's way in, and an agent's started again only stops once its
firings have ended, so none is fired twice.

## How does a subagent get fewer tools?

A subagent is a new conversation of the same agent, so by itself it has
every tool its parent has. Narrowing it is a list per conversation, in
the harness contract (`tools`): the subagent's task comes with
`Link-Tools`, the names of the tools it may use, and the harness keeps
that list with the conversation, offering it only those from then on.

### Never widened

A later list is intersected with it, so nothing widens a conversation
again. The harness tells its tools which a conversation has (`_meta`
`link.local/tools`), so the scheduler gives a subagent its parent's
tools or fewer, a narrowed parent's subagent included, and never more.

### Additive

It's additive: a harness that doesn't say `tools` gives a subagent all
of them, as before, and a list is refused for it rather than ignored,
since a subagent with tools it was meant not to have is worse than one
not started. The mini harness answers it and the conformance kit checks
it.

### Where Link Harness keeps it

Link Harness keeps the list beside the conversation's events, not in
them: it's a grant, like the agent's settings, and finding it in the
events would read every event of every conversation at each turn.
