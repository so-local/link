# Decisions

Why `lnk measure` works the way it does.

## The module and its command

Why the watcher is a module of its own, and its command a group beside
[`lnk agent`](../../../../../agents/agent/docs/agents/README.md#agents).

### Why is the watcher a module of its own?

It measures from outside, around the programs it measures as the
sandbox is around them, and never part of them: the harness doesn't
report its own memory, and an agent on OpenClaw or Hermes is measured
as one on Link Harness is. Neither the harness module nor a harness
owns that, and the machine is measured with no agent at all. It finds
what to measure by asking: the modules built on it answer its questions,
`measure.agents` and `measure.traffic`, in its contract's types, so it
names none of them.

### Why is the command `lnk measure`, a group of its own?

The machine is measured as well as its agents, and the watcher, its
settings and the export are the measure module's, not an agent's. So
it's a group of its own, beside `lnk agent`, and [`lnk agent list`](../../../../../agents/agent/docs/agents/start/create.md#run-more-than-one-agent)
shows the two numbers you'd look for there: memory and the day's
spend.

## What a measure is

What a measure and a unit are, and how they're kept on the machine and
sent from it.

### Why OTLP?

Every observability tool reads OpenTelemetry's protocol, so Link's
measures go to Grafana, Honeycomb or your own collector as they are,
with nothing of Link's to install there. It's also how a harness
reports what only it can count.

### Why is a measure's rollup in its name?

`link.memory.used.sum`, `link.memory.peak.max`, `link.time.turn.own`:
whatever reads a measure knows, from its name alone, how a unit's adds
up to its parent's (summed, the largest, or not at all), so a
dashboard of agents sums to the machine's without knowing Link.

### What is a unit?

The machine, an agent, or one of an agent's processes (its sandbox's
`bwrap` or `sandbox-exec` is the kind `sandbox`), each by the ID it
has: the machine's, the agent's, and `<agent id>/<pid>` for a process.
An agent from before IDs is `<installation id>:<name>`. A process is listed
under its agent, not its parent process: what an agent costs is the sum
of them.

#### An agent's owner and teams

An agent's owner is an attribute of its own (`link.owner.id`), so a
dashboard can group agents by whose they are, and so are its teams
(`link.team.ids`, the IDs of the teams whose files list it, an array
since an agent can be in several), so one can group them by team.

### Why are teams sent by their IDs, not their names?

A team's name is text a person chose, and the measures carry as little
of anyone's words as they can. An ID says which team and nothing more,
and a dashboard groups by it as well. OpenTelemetry's conventions have
no attribute for teams, so it's Link's own, `link.team.ids`.

### Why is the store on the machine not OTLP?

OTLP's JSON spells each measure's name, unit and kind out at every
point, about eight times the disk of the numbers. The watcher keeps
each look as one short line of numbers by unit and measure, and
speaks OTLP where it crosses programs: from a harness, and to your
endpoint. A look keeps the machine and each agent, and each process
only when your settings say (`processes`, off in Link's), since it
multiplies the disk by an agent's processes: a trade, so a setting.

### Why are a file's values by order, with so much left out?

Each file names its measures once, in its first line. Values follow that
order, omitting zeroes, unchanged totals and repeated unit descriptions.
A reader carries the omitted state forward, reading from the start or
from the checkpoint in `day.json`. Completed hours and days are compressed
with zstd. A value that stays zero all day is shown as absent.

### How long is it kept?

A trade, disk against history, so it's the watcher's settings, not a
constant: Link's (a look every 10 seconds, each kept a day, a minute's
summary kept 30 days) are a file the module comes with, and yours win where
you set them. They're the machine's, not an agent's: every agent here
shares one watcher.

### Why does each home name its own service?

launchd and systemd keep one service per name for each user, so a fixed
name let a second Link home of one user replace the first's watcher. The
label is `link.measure.<key>` and the unit `lnk-measure-<key>.service`,
where the key is a digest of the home's resolved path. Moving a home
therefore names a new service.

### What happens to a service from an earlier version?

An earlier `lnk` used `link.measure` and `lnk-measure.service`. The next
`lnk measure start` or `lnk measure stop` stops the old service and removes
its file, but only when that file is in this home, so a home that never
ran one leaves another's alone. Until then the old service keeps running,
so one home never has two watchers.

### Why is the export OTLP over HTTP as JSON?

gRPC and protobuf would add dependencies to Link that every collector
already spares: they take OTLP's JSON over HTTP, with headers you set
for a hosted one's key. It's one POST of the same data to
`<url>/v1/metrics`.

## Where each figure comes from

Who may say each measure, and how each reaches the watcher.

### Why does a harness report through a file?

It runs in its sandbox, which writes only its home and reaches no port
of this machine's. So [`lnk agent start`](../../../../../agents/agent/docs/agents/README.md#create-an-agent) names a file in its home
(`LNK_MEASURES`), the harness appends a line of OTLP a turn, and the
watcher reads on from where it left off. The harness moves it aside
past 1 MiB, so it never grows.

### Who may say which measure?

Each measure has one writer, and a file gives only what its writer may
say. What's measured from outside (memory, CPU, GPU, disk, network,
processes, ports) is the measure module's alone. A harness says only
what it counts itself, in its home's file: a turn's tokens, cost,
calls and times. The harness module says only how long the starts and
stops it made took, in the agent's settings. So a harness can't claim
to have started in no time, nor the harness module a turn.

### Why are starts and stops timed by the harness module?

A harness can't time its own start: it isn't running until it is, and
its stop ends it. The harness module starts and stops every harness,
so it times each, from its service started to its harness answering
its health check, and from the stop asked until its main processes
are gone. It appends each as a line of OTLP to `times.jsonl` in the
agent's settings (`~/.lnk/safe/agents/<name>/`), which no harness
writes, and the watcher reads it as it reads a harness's file. A file,
not a message to the watcher, so a start is timed whether the watcher
runs or not.

#### Cold, warm and the system's starts

A start is from cold when nothing of the agent was timed since the
machine started, and warm otherwise. A start the system makes itself,
at boot or login, isn't timed, since no `lnk` runs it and nothing waits
for its answer. A stop is waited on 10 seconds at most, so
[`lnk agent stop`](../../../../../agents/agent/docs/agents/harnesses/use/README.md#one-running-in-a-terminal) is never held up by one that hangs; one longer isn't
kept.

### Why is the file emptied once it's read?

The harness appends measures and the watcher reads them under the same
exclusive file lock. Once the watcher has consumed every complete line, it
empties the consumed file, bounding disk use without changing the wire
format. A partial final line remains for the next read. The lock prevents
a writer's new line from being removed between the read and truncation.

### Why does network come from the proxy?

An agent's traffic leaves its sandbox only through its proxy, which
counts every byte each way for the wire's log already. So it's what
crossed the proxy, not a guess from the machine's interfaces, which
every program shares. A proxy beside an unconfined harness counts only
what passes through it, since that harness can bypass it.

#### Published snapshots

The provider advertises its existing private snapshot directory in the agent
listing, using the measurement contract's type. That keeps collection below
the provider's implementation, avoids repeated discovery and command startup,
and leaves publication and cleanup with their owner. Each used sample reads
once, without a cross-sample cache. Providers without snapshots keep their
command protocol, discovered with the existing agent-list refresh.

## Tokens

Why every agent's tokens are counted at its proxy, and how, so harnesses
compare.

### Why do tokens come from the proxy?

Only Link Harness counts its own tokens; OpenClaw, Hermes and DeepSeek
Harness don't say theirs. Every agent's model requests already pass
through its proxy, which adds the key, and every provider's answer
says what it took. So the proxy reads that usage as the answer passes,
and every agent's tokens are counted the same way, as the provider
counts them, whichever harness it runs. It's read as the answer goes
by, never held back: a stream's events one at a time, looked at only
when one names usage, and a whole answer scanned for its top-level
`usage` without being kept.

#### Read inline

Reading runs inline, as the proxy writes each piece to the program:
a stream's event is held only until it ends, at most 4 MiB (one past
it isn't read, and its request counts as one without usage), and a
whole answer only its top-level usage, one value a key.

### Why one source for tokens?

Link Harness reports its tokens too, and its requests pass through its
proxy: counting both would count each token twice. An agent whose
proxy carries its model takes its tokens from the proxy alone, and
from its harness everything else (turns, spend, a turn's times). An
agent with no sandbox has no proxy, and its harness's count is the one
there is. The proxy's is the one kept where both are, so every agent
is counted alike. The watcher remembers which agents their proxy
counts, as each last ran, so a turn reported after the agent stopped
isn't added on top.

#### What it costs

The cost: an agent counted by its proxy loses the tokens of any model
it reaches some other way, such as a hosted API through a plain
`CONNECT` with a key of its own, which the proxy can't read.

### Why is a stream without usage counted, not changed?

An OpenAI Chat Completions stream gives its usage only when its
request asks. The proxy could add the ask, but a request the harness
didn't send could change the answer it gets: a last event it doesn't
expect. So nothing is added, and an answer that gave no usage counts
as a request without it (`link.requests.unmetered.sum`), so a gap is
seen as one rather than as fewer tokens.

### Why are a model's answers asked for uncompressed?

A compressed answer can't be read as it passes without unpacking it,
which a proxy that only passes it on shouldn't do. So a request for a
model's answer asks for it uncompressed (`Accept-Encoding: identity`),
which providers honor and a harness reads either way; a model's
answer is text, and small beside what a harness downloads. One that
comes compressed anyway counts as a request without usage.

### Are a local model's tokens counted too?

Yes. A local runtime's requests go through the same proxy, by the local
`model` verb, so a local agent's tokens are counted as a hosted one's:
OpenAI's format on `/v1` and Ollama's own on `/api/chat` and
`/api/generate`. It also lets the counting be tested without a
provider's host.

### How are tokens counted, so harnesses compare?

As Link Harness counts them: `input` is all of the prompt (Anthropic's
`input_tokens`, cache reads and cache writes together; OpenAI's
`prompt_tokens`), and `cached` is what was read from the provider's
cache. Cache writes are a measure of their own,
`link.tokens.cache_written.sum`, since Anthropic charges a write above
the price of input and a price made from `input` alone would be wrong.
A model request isn't a turn: for a harness that isn't Link Harness,
`link.requests.model.sum` counts them and turns stay Link Harness's.

## Processes and the machine

How the machine and an agent's processes are read, on Linux and on a
Mac.

### Why is GPU time counted since the last look?

A GPU says how busy it is now, not how much it was used since it
started, and NVIDIA's says nothing of a process's time. So the
machine's GPU time over a look is how busy its GPUs were at the look
before, times the look's length: GPU-seconds, summed as CPU's
core-seconds are, with two GPUs at half counting as one. A process's
is the busy time its kernel counts on its GPU (Linux's DRM
`fdinfo`), which it grew by since the last look, and an agent's adds
its processes'.

#### Sent as a delta

Each is sent as a delta since the last look, as a
proxy's tokens are, and the watcher keeps what each process had at its
last look, by its pid and start, so a restart counts none of it twice.
On NVIDIA a process's memory is read, not its time.

### How is a Mac's machine read?

A Mac has no `/proc`. Its CPU is the cores' ticks since it started
(`host_statistics`), its network each interface's bytes (`netstat
-ibn`), its ports those listened on (`netstat -anp tcp`, and for a
process, `lsof`, which sees this user's processes), and its GPU what
`ioreg` says of each accelerator. Each command is run by its full path,
at most 5 seconds, and only its numbers are read; what each prints is
tested as recorded from a Mac.

### Why are disk read and written the control group's?

On Linux, an agent's service's control group counts every process it
ever started, even those that exited between two looks, as a turn's
tool server does. A Mac has no control group, so there it's the
processes running at the look.

### How is an agent's memory counted?

An agent is measured from outside, so every process it runs counts,
whichever harness it is. On Linux that is its service's control group,
which holds every process it starts, and on a Mac the tree under its
processes. Memory in use is PSS on Linux, where pages shared between
processes are split between them, so a sum doesn't count a shared page
twice. On a Mac it is the footprint, as Activity Monitor counts it.

### Why is a process named only by a program Link knows?

What a system calls a process is what the process chose: a script's
file name, which an agent may take from a conversation, or any name a
program sets itself. A name taken from it would carry those words into
what's shown, kept and sent, and give a harness with no network a way
to write into your endpoint. So a process is named only by one of a
short list of programs (Link's own, the sandboxes', and what harnesses
run on), and any other only by its ID.

### Why is a harness's point capped?

What a harness reports of itself is its own say, so it can claim
anything: a turn of 1e308 tokens. Two such points sum to infinity,
which JSON can't write, and the look or minute holding it would read
back as nothing, every agent's with it. So a point past what a turn
could be is dropped, and every sum stops at the largest number there
is.
