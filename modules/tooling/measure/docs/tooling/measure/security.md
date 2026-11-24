# Security

What `lnk measure` reads, keeps and sends, and what it never does.

## Never content

A measure is a number, never a message, prompt, file, tool argument or
answer: each part of what's measured is held to that.

### A measure and its unit

A measure is a number of a measure Link names, of a unit named by an
ID, a kind, its parent's ID and a name: an agent's or machine's name, or
a process's program. An agent's owner and teams are their IDs. There's
no field for text. A name or ID is kept only if it's letters, digits and
`._:/@-`, at most 64 of them.

### A process's name

A process is named only by a program on Link's list (`lnk` and its
modules, `bwrap`, `sandbox-exec`, `node`, `bun`, `deno`, `python`,
`python3`, `uv`); any other has no name, only its ID. What a system
calls a process is what it chose: a script's file name, or anything it
set itself, so a name taken from it could carry a conversation's words,
or a sandbox's, to wherever measures go.

### Names that go with them

An agent's and the machine's names are yours, and go with their
measures to your endpoint, as does each agent's owner (`link.owner.id`):
your account (`acct-<id>`), or `<user>@<machine>` without a login.

### What a harness reports

What a harness reports of itself is read as the same schema, and
everything else is dropped as it's read: a measure Link doesn't name, a
description, an attribute, text where an ID goes. So no message,
prompt, file, tool argument or answer reaches what's kept, shown or
sent, even from a harness that wrote one there.

#### What a harness may say

A harness may report only what it counts itself (a turn's tokens, cost,
calls and times); what's measured from outside, and what only its proxy
counts (`link.requests.*`, and the tokens written to the provider's
cache), it can't say.

#### How many values it keeps

Of each measure kept one value at a time (a turn's times), a look takes
at most 64 of a harness's, and a minute's summary and each hour of the
day keep at most 64 more, a spread of them standing for the rest, so a
harness can't fill the disk or the watcher's memory with them.

### An agent's tokens

An agent's tokens are counted by its sandbox's proxy, from the usage
each model's answer gives as it passes: six counts a proxy (requests,
those without usage, and input, output, cached and cache-written
tokens), never a word of a prompt or an answer.

#### What the proxy reads of an answer

It reads at most 4 MiB of a stream's event and one value of each usage
key of a whole answer: only the events that name usage, and a whole
answer's top-level `usage`, and keeps their numbers. A count past a
billion tokens is taken for what isn't so, and counted as an answer
without usage.

### Link Harness's measures

Link Harness derives its measures from a turn's events, numbers only. A
test sends a turn with a known phrase, through a tool call and its
answer, and fails if its measures hold any of it.

#### The tests that hold it

Another test writes the phrase everywhere a harness could, and fails if
anything kept, shown or sent holds it; a third sends the phrase to a
model through an agent's proxy and back, and fails if anything counted,
kept, shown or sent holds it.

## What it reads

What it reads of the machine, its agents, their proxies and their
harnesses, each from outside the agent.

### Each agent's processes

As this user, from outside: on Linux `/proc` and its service's control
group, on a Mac `libproc`. Nothing runs inside an agent's sandbox, and
no harness is asked anything. Linux kernels without per-thread
`children` files are read by the parent process IDs in `/proc/<pid>/stat`
instead.

### The agents and their traffic

The agents here, their processes and where their harnesses report, from
the module that answers `measure.inventory`, falling back to the unchanged
`measure.agents` answer for an older or unavailable provider. The inventory
names published private traffic snapshots. Measure reads those through
the shared typed contract, without commands or writes, checking live PIDs and
their start times where available. They hold bytes each way, never where to.

#### Providers without snapshots

A provider without a published source still answers `measure.traffic` through
its command. Agent and traffic provider discovery share the agent listing,
refreshed every minute or when a running agent exits. A sample reads traffic
only when a running process needs it, once for all consumers in that sample.

#### Snapshot ownership

The proxy still writes private snapshots at its existing cadence and removes
its own file on exit. It and the sandbox command clean dead or reused PIDs'
files. The observer never removes a publisher's state, creates its directory,
changes permissions or substitutes traffic telemetry for authorization.

### Each agent's model tokens

As its model route counted them, from
`~/.lnk/safe/measures/credentials/<agent ID>.json` (600, numbers only, never a word
of a prompt or an answer). A proxy's file counts for the process of its
number only when it names that process's start, where the start can be
read (Linux), so a file left by a proxy that crashed, or put there,
isn't credited to a process given its number later.

### GPUs

Each machine's GPU, where it has one: `nvidia-smi` (on the `PATH` or in
`/usr/bin`), `/sys/class/drm`, each agent process's `/proc/<pid>/fdinfo`
for its descriptors on `/dev/dri`, and on a Mac `ioreg`.

### A Mac's machine

On a Mac, its CPU from `host_statistics`, its network and ports from
`netstat`, and which of this user's processes listen from `lsof`. Each
command, a GPU's too, runs at most 5 seconds and only its numbers are
read.

### Start and stop times

The file of an agent's start and stop times, `times.jsonl` in its
settings, which the harness module writes and no harness can: only its
starts' and stops' times are taken from it.

### A harness's file

The file a harness reports to, in its home, which the harness can
write: it's opened without following a link, only if it's a plain file,
at most 4 MiB a look, and a line longer than 64 KiB is skipped. A line
longer than a look reads is skipped too, to its end however many looks
that takes, so it never holds up the lines after it.

## What it keeps

What stays on the machine, who reads it, and how it's deleted.

### Selected Job Profiles

An explicit job selection asks its owning module for a retained profile.
Reading never consumes or rewrites it. Profiles must be regular files,
opened without following a final link, at most 64 MiB with lines at most
64 KiB. Malformed or incomplete profiles fail. Unknown units, unrelated
jobs and measurements outside the numeric schema cannot be shown or
exported. Export sends a selected profile only on an explicit request.

### `~/.lnk/safe/measures/`

Only yours (`700`, files `600`): the watcher's looks, a day of them, and
a summary of each minute, 30 days; old files deleted whole. Each agent's
processes are shown and sent, not kept, unless your settings keep them
(`lnk measure set --processes`). `lnk uninstall --delete
measures` deletes the folder.

### `counted.json`

`counted.json` there keeps what each running proxy had counted at the
last look, and which agents their proxy counts, so a restarted watcher
counts none of it twice.

### Your endpoint's headers

Your endpoint's headers (an API key) are in `settings.toml` there, which
only you read, and never shown: `lnk measure get` says how many there
are. The watcher's log names only the endpoint's host.

### Your endpoint's URL

An endpoint's URL can't hold a login (`https://user:key@host`), a query
or a fragment: `lnk measure export set` refuses it, and says to use
`--header-file` for a login, as `settings.toml` written by hand is
refused with one.

## What it sends

Nothing, until you name an endpoint (`lnk measure export set --url <url>`). Then
every look goes there as OTLP, with your headers, at most 10 seconds a
request, over `https://`, or `http://` to this machine alone: a plain
`http://` endpoint anywhere else is refused, where it's named and where
it's read, so your headers never cross the network in the clear. Local
Link never receives it.
