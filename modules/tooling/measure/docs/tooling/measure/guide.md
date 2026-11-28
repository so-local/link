# Measure Guide

Inspect current resource use, retain a history, and send numeric measurements
to your observability tools. On macOS or Linux, start with a live snapshot,
then enable the watcher when you need historical views.

```sh
lnk measure
lnk measure --agent main
lnk measure start
```

## Resources

Inspect machine capacity and the resources used by agents through the same
command family.

### See what your agents use

`lnk measure` shows the machine, each agent here and each of an agent's
processes, with the memory each holds now, the CPU it has used since it
started, what crossed its proxy, and its tokens, turns and spend over
the last day. [`lnk agent list`](../../../../../agents/agent/docs/agents/start/show.md#list-your-agents) shows each agent's memory and the day's
spend too, as the watcher last saw them (none while it isn't running).

#### Measured from outside

Each agent is measured from outside, process by process, whichever
harness runs it, so OpenClaw, Hermes and DeepSeek Harness are measured
as Link Harness is. [Its tokens are counted](#tokens) from its model's answers as
they pass its proxy.

#### What only Link Harness reports

How long it takes to start and stop is timed by [`lnk agent`](../../../../../agents/agent/docs/agents/README.md#agents) itself,
which starts and stops every harness. Only Link Harness reports what a
harness alone can count, which is a turn's cost, and the time until the
model is asked and until the first word of the answer.

### Inspect Machine Capacity

Read a fresh resource snapshot without starting the watcher. JSON includes
CPU model, logical and physical cores, affinity and quota limits, usable cores,
available clock information, memory capacity and limits, and filesystem space.

```sh
lnk measure show --json
lnk measure show --path /tmp
```

Host capacity and limits are separate. A container may see many host cores
while its usable allocation is smaller. Filesystem observations distinguish
total, free and user-available bytes. Missing paths and unavailable OS fields
remain unknown. The snapshot includes its date, OS, architecture, kernel and a
machine fingerprint independent of the current user or execution profile.
Cloned machines need distinct OS identities or hostnames to be distinguishable.
macOS reports available sysctl capacity but leaves unavailable memory headroom
and clock values unknown.

#### Finite Executions

The shared Measure contract also supports finite owned executions. Callers
supply run, task and process identities, then retain numeric OTLP observations
beside their own results without stream rotation or an automatic export.

### Cloud and box coverage

The live view measures this machine and its agents. A retained job profile
is read separately through [Job Profiles](#job-profiles). Measure does not
currently discover all boxes or combine connected clouds into a dashboard.
Machine readings, use inside a guest and a VM's reserved capacity have
different scopes. Measured use does not enforce a workload limit or replace
the executor's resource reservations.

### See one agent over a day

This shows every measure now and over the last day, the day an hour at a
time, and the agent's processes. Over a day, memory is the most it held,
CPU and network are what they grew by, tokens, turns and spend are their
sum, and a turn's times are their median. History is what the watcher
kept, so start it first.

```sh
lnk measure --agent main
lnk measure --agent main --pid 4567
```

## Keep watching

Start the watcher to record a snapshot every 10 seconds. It retains snapshots
for a day and minute summaries for 30 days in `~/.lnk/safe/measures/`,
for the machine and each agent. Agent processes are shown and sent but not kept.

```sh
lnk measure start
lnk measure stop
```

### Its settings

These are Link's settings, and you can change them to keep more or less.

#### Show its settings

`lnk measure get` shows each setting, whether it's yours or Link's, and
where else the watcher sends its looks.

```sh
lnk measure get
```

#### Change its settings

`lnk measure set` writes the settings it's given, then shows them all.

```sh
lnk measure set --keep-fine 6h --keep-minutes 7d
lnk measure set --every 30s
lnk measure set --processes
```

## Send them to your observability tools

Send watcher snapshots to an OpenTelemetry collector using OTLP
(OpenTelemetry Protocol) over HTTP. The collector receives JSON at
`<url>/v1/metrics`, so Grafana, Honeycomb, or your own service can read them.

```sh
lnk measure export set --url http://localhost:4318
lnk measure export set --url https://api.honeycomb.io --header-file honeycomb.headers
lnk measure export set --off
```

Each says which of the three it does: `--url` sends to an endpoint,
`--off` stops sending, and `--job` sends one job's profile. `lnk measure
get` says where they go now.

### Where it may send

The endpoint is `https://`, or `http://` to this machine (`localhost`,
`127.0.0.1`, `[::1]`): a plain `http://` endpoint anywhere else, such as
a collector on another machine or in another container
(`http://otel-collector:4318`), is refused, since your headers would
cross the network in the clear. Give that collector a TLS receiver, or
one on this machine to forward to it.

### Each unit

Each unit is a resource, named by `link.unit.id`, `link.unit.kind`
(`machine`, `agent`, `sandbox`, `process`, `job` or `stage`; `relay`
and `scheme`, for what a relay sends of itself and of each naming scheme;
or `provider`, a model's provider, named as Link's model facts name it), `link.unit.parent`, `link.unit.name`,
`link.machine.id` and, for an agent, `link.owner.id`, whose it is, and
`link.team.ids`, the IDs of the teams it's in (an array of strings),
when it's in one.

### How a measure adds up

The last word of a measure's name says how it adds up to the unit above
it. `sum` adds, `max` takes the largest, and `own` doesn't add at all.

## What's measured

Each measure, by its name, with its unit. Turns, spend and a turn's
times come from Link Harness alone, and another harness is measured
from outside, its tokens included.

### Memory and CPU

`link.memory.used.sum` and `link.memory.peak.max`, in bytes: in use now,
and the most seen. `link.cpu.used.sum`, in core-seconds: used since it
started.

### GPU

`link.gpu.used.sum` and `link.gpu.memory.sum`, in GPU-seconds and bytes:
GPU time used since the last look, and GPU memory in use, [where there's
a GPU that says](#gpus).

### Disk

`link.disk.used.sum` and `link.disk.files.sum`, in bytes and files: an
agent's folders, looked at every 10 minutes. `link.disk.read.sum` and
`link.disk.written.sum`, in bytes: read from and written to disk.

### Network, processes and ports

`link.network.in.sum` and `link.network.out.sum`, in bytes: through an
agent's proxy, or the machine's own network. `link.processes.sum` and
`link.ports.sum`, a count: running, and TCP ports listened on.

### Turns, tools and errors

`link.turns.sum`, `link.tool_calls.sum` and `link.errors.sum`, a count:
turns answered, tools called, and what failed.

### Fallbacks

`link.fallbacks.off.sum`, `.window.sum` and `.overloaded.sum`, a count:
turns the agent's fallback model answered (`lnk agent start
--model.fallback_model`), by why: its model was off, the turn's prompt was past its
model's window, or its model's provider answered busy or failing (429
or a 5xx).

### Tokens and model requests

`link.tokens.input.sum`, `.output.sum`, `.cached.sum` and
`.cache_written.sum`, in tokens, as the provider counts them: the prompt
(cached or not), the answer, and of the prompt, what was read from the
provider's cache and what was written to it (Anthropic's, which it
charges above the prompt's price).
`link.requests.model.sum` and `link.requests.unmetered.sum`, a count:
requests for a model's answer its proxy carried; of them, answers that
gave no usage, whose tokens aren't counted.

### Cost and a turn's times

`link.cost.sum`, in micro-dollars: what the provider said a turn cost,
never estimated. `link.time.to_model.own`, `.first_token.own` and `.turn.own`, in
seconds: from a message in to the model asked, to the answer's first
piece out, to the whole answer.

### Said by a relay or a service

A relay and a service that units are reached through each say what
passed through them, beside what the units say of themselves.

#### A relay's

`link.relay.connections.sum`, a count of relay agent connections,
including standbys, and `link.relay.unadmitted.sum`, requests refused
before routing. `link.relay.requests.sum`, `.refusals.sum`,
`.failures.sum` and `.bytes.sum` count requests, the relay's own 4xx
and 5xx answers and the encoded payload bytes, each by the naming
scheme that took them. `link.hooks.undelivered.sum`, a relay's,
never a machine's, counts webhooks it couldn't hand on through its
tunnel (the
relay).

#### A service's

A service a unit is reached or served through says these of each unit
it serves, never a unit of itself: what passed through it for that unit.

- **Messages:** `link.messages.arrived.sum`, and of them `.answered.sum`,
  `.queued.sum` and `.refused.sum`, a count: so a lost message shows as
  more arrived than answered, held or refused. `link.notices.sum` counts
  the notices its owner saw from Link rather than from the unit itself.
- **What it was charged:** `link.cost.charged.sum`, in micro-dollars:
  its model requests at the price its owner paid, beside
  `link.cost.sum`, the provider's.
- **Ceilings:** `link.refusals.ceiling_hour.sum`, `.ceiling_day.sum` and
  `.ceiling_new.sum`, a count of model requests refused at a spending
  ceiling, for the hour, the day, or the day new accounts share, each
  under its `provider` unit.
- **Storage:** `link.storage.home.sum` and `.database.sum`, in bytes:
  its Home's files and its rows where they're stored. `link.decrypts.sum`
  counts what a key management service decrypted for it.

### Starts and stops

`link.time.start_cold.own`, `.start_warm.own` and `.stop.own`, in
seconds: a start from cold (an agent's first since the machine started)
or warm (any start after), until its harness answers, in the
background or in a terminal; a stop, until none of its main processes
is left, or for one in a terminal, [`lnk agent stop`](../../../../../agents/agent/docs/agents/harnesses/use/README.md#one-running-in-a-terminal)'s until it has
ended.

#### When they're timed

Starts and stops are timed for every harness, each time `lnk agent`
starts or stops it with `start`, `restart` or `stop`, and each time [`lnk up`](../../../../../../docs/more/modules/install.md#pick-your-modules) does: a start
the system makes itself, as a machine starts or you log in, isn't
timed. A stop that takes more than 10 seconds isn't kept.

## GPUs

Where a machine has a GPU that says what it uses, the watcher reads it.
Without a GPU, or with one that says nothing, no GPU measure is shown.

### NVIDIA

`nvidia-smi` gives each GPU's use and memory, and each process's memory
on them. A process's GPU time there isn't said, so only the machine's
is.

### Linux's own drivers and a Mac

For AMD, Intel and others: each card's use and memory in
`/sys/class/drm`, and each process's time and memory on its GPU in
`/proc/<pid>/fdinfo`, as the kernel counts them. On a Mac, `ioreg` gives
the GPU's use and memory, the machine's only.

### How GPU time adds up

The machine's GPU time over a look is how busy its GPUs were times the
look's length, so two GPUs at half count as one. A process's is what
its kernel says it grew by since the last look, and an agent's is its
processes' added up.

## Tokens

Every agent's tokens are counted as the provider counts them, whichever
harness it runs. Its model's requests pass through its sandbox's proxy
to the proxy's model route, served by the models module outside the
sandbox, which adds the key, and reads the usage each answer gives as
it passes it on, never holding it back or changing it.

### Anthropic

`usage` in a whole answer; in a stream, `message_start`'s and
`message_delta`'s. The prompt is its three parts: what wasn't cached,
what was read from the cache (cached) and what was written to it (cache
written), as Link Harness counts it.

### OpenAI, OpenRouter and local runtimes

For OpenAI (Chat Completions and Responses), OpenRouter, and a local
runtime's OpenAI-compatible API: `usage`, in a stream the last event's.
Ollama's own API gives `prompt_eval_count` and `eval_count`.

### A stream without usage

A Chat Completions stream gives its usage only when its request asks
for it (`stream_options.include_usage`), and the proxy changes no
request. An answer without one counts in `link.requests.unmetered.sum`,
so a gap shows as one rather than as fewer tokens.

### Where they're added up

The route adds each answer's counts to the agent's, a file of the
measure module's in Link's settings (`~/.lnk/safe/measures/credentials/<agent ID>.json`),
which the watcher reads.

### An agent with no sandbox

An agent with no sandbox has no proxy, so its tokens are what its
harness reports, which Link Harness does. A harness's own count of the
tokens its route counts is never added too, so nothing is counted
twice. Spend is never estimated from tokens: it's what Link Harness
says the provider charged.

## For scripts

Each measure is `{"now", "day"}`, its value now (`null` for what's
counted per turn) and over the last day. The
[contracts](../../../contract/docs/tooling/measure/more/protocol.md#lnk-measure)
page has the rest.

```sh
lnk measure --json
lnk measure --agent main --json
```

### Job Profiles

Read a retained job profile by its job ID. This asks the job module for
the profile and shows the job with its child stages, without running a
watcher or changing the retained file. Missing profiles are errors and
unavailable measurements stay absent. Job totals are lifetime totals.
Stage counters are snapshots of the job's cumulative counters at that
stage, while memory shows the largest observation.

```sh
lnk measure --job <id> --json
```

#### Export a Job

After configuring an endpoint with `lnk measure export set --url <url>`,
send a retained job profile once. The command waits for the endpoint's
acknowledgement and reports a failed delivery. It does not change the
watcher's export settings or automatically resend old jobs.

```sh
lnk measure export set --job <id>
```

## Where things live

The watcher's files are all under `~/.lnk/safe/measures/`, except what a
harness reports of itself and what `lnk agent` timed. `settings.toml`
there holds your settings, and your endpoint and its headers;
`watch.log` is the watcher's log.

### What the watcher kept

`~/.lnk/safe/measures/fine/` and `minutes/` hold what the watcher kept, a file
an hour and a file a day: `<date>-<hour>.jsonl` while it's written,
compressed to `.jsonl.zst` (zstd) once its span is over.

### Each file's lines

Each file is lines of JSON. Its first names its measures,
`{"measures": ["memory.used.sum", ...]}` (`link.` left out); each line
after is one look, `{"t": <seconds since 1970>, "units": [...]}`, each
unit its `id`, and its values in the order the first line names (`"v":
[...]`, `null` for one left out). What's counted per turn is `"e"`,
each measure's values by its place in that order.

### Values left out

A value is left out when it's zero, but a running total's: a total is
kept, zero too, and left out only when it hasn't changed since the
unit's last line in the file. So a day, read back or as the watcher
keeps it, has no zero but a total's, and `lnk measure --json` shows the
same either way.

### A unit's names

A unit's `kind`, `parent`, `name`, `owner`, `teams` and `s` (when its
totals began, in nanoseconds) are written on its first line in the file
and when they change. A reader carries each forward from the unit's
line before.

### `now.json` and `day.json`

Beside them, `now.json` is the watcher's last look and `day.json` the
last day as it summed it up, each minute, so `lnk measure` reads only
the looks since rather than the whole day.

### Outside `~/.lnk/safe/measures/`

In each harness's home, `.lnk/safe/measures/measures.jsonl` holds what it reports of
itself, for the watcher to read: emptied once the watcher has read it
all. In each agent's settings, `~/.lnk/safe/agents/<name>/times.jsonl` holds
how long each of its starts and stops took.

## More about it

Where the rest of what `lnk measure` does is written:

- Every flag is in `lnk measure --help`.
- [Security](security.md) has what's kept and sent, and what never is.
- [Decisions](decisions.md) has why it works this way.
- [Troubleshooting](troubleshooting.md) has what to do when a number is
  missing.
