# Interface

What other programs can rely on from `lnk measure`.

## Local observation contracts

The Rust client contract also defines published traffic and controlled phase
records. These are reusable data boundaries, separate from the command's
normal live view and from the fixture that starts a measured workload.

### Published traffic

`measure.inventory` carries `agents` and published `traffic` sources, each a
`provider` and absolute `directory` of typed per-PID snapshots. The shared
reader validates process identity and reports stale paths to the owner.
Measure reads without mutation, once per used sample. Other providers retain
their `measure.traffic` command fallback, discovered with the agent listing.
The unchanged `measure.agents` answer remains the fallback for an older or
unavailable inventory provider, preserving strict older readers.

#### Traffic guarantees

A sample with no running agent makes no traffic query. A used sample reads
published counters once, shared by every consumer in it, and the next sample
reads fresh ones. Dead, mismatched or corrupt snapshots are never credited,
and counter correctness stays separate from how many commands a sample starts.

### Controlled phase records

`client::record::Phase` records explicit roots, observation time and a Linux
process tree with PID/start identity, parentage and known program labels.
CPU includes waited children and cannot be summed as disjoint tree use.
`Executions::in_window` counts successful images in a supplied process trace,
including resumed execs, with explicit observer exclusions. Missing or malformed
traces yield unavailable counts. Neither API starts a target or tracer.

### Live Process Observations

On Linux, live process observations use per-thread child lists when the kernel
provides them. Otherwise, one process inventory supplies parent relationships
for the whole observation. The next observation reads a fresh inventory.
The fallback reads parent fields directly, without allocating a full set
of process counters for every entry in that inventory.
Exited processes are excluded from the live tree, while an owned root's final
counters remain readable before its owner reaps it.

### Retained Job Profiles

`measure.job` accepts `client::asked::Job`, containing a job `id`, and
returns its retained profile path or `null` when no profile exists, [each
in its envelope](../../../../../../../../core/framework/docs/more/modules/protocol.md#questions).
The owning module retains the file. An observer opens a regular file
without following its final link and validates its OTLP before use.
The closed unit kinds include `job` and `stage`, with each stage's
`parent` naming its job. Unknown kinds remain invalid.

### `lnk measure --job <id> --json`

Returns `at` in Unix nanoseconds and `units` for the job and its stages.
Each unit has `id`, `kind`, `parent`, `name` and `measures`. Each measure
contains `value` and its observation's `at`, also in nanoseconds.
Counters use the latest sample and memory uses the largest observation.
Only valid units belonging to the selected job reach the view or export.

## `lnk measure`

The commands other programs read, and what each prints or does.

### `lnk measure [<unit>] --json`

Prints `{"at", "units": [{"id", "kind", "parent", "name", "owner",
"teams", "measures": {"<measure>": {"now", "day"}}}]}`: this machine,
each agent here and each of its processes, or the unit named and its
processes. `at` is in Unix seconds. Read by [`lnk agent list`](../../../../../../../agents/agent/docs/agents/start/create.md#run-more-than-one-agent).

#### Each unit

`kind` is `machine`, `agent`, `sandbox` or `process`, and a process's
`id` is `<agent id>/<pid>`. `owner` is an agent's owner's ID, and
`teams` the IDs of its teams (`[]` for a unit in none).

#### Each measure

Each measure is one of `link_measure_contract::client::MEASURES`, in [`measure.rs`](https://github.com/so-local/link/blob/main/modules/tooling/measure/contract/src/lib.rs),
its value now and over the last day, as the watcher kept it (`null`
without); one with neither is left out.

#### What has no value now

`now` is `null` for what's counted as it happens: a harness's per turn,
a start's or stop's time, tokens, model requests and GPU time.

#### One unit's hours

For one unit, `"hours": [{"at", "measures": {"<measure>": <value>}}]`,
the last day an hour at a time, those with anything kept.

#### `--last`

Hidden: only the watcher's last look, however old, never one taken now,
or `null` when no watcher runs. `lnk agent list` reads each agent's
`link.memory.used.sum` now and `link.cost.sum` over the day from it, by
its ID.

### `lnk measure start --quiet`

The watcher runs in the background once it exits; nothing said, and
nothing done where background services are off. Read by `lnk agent
start`.

## Foreground Watcher

`lnk measure watch` is the foreground entry used by the background service.
For a development session with `LNK_SERVICES=off`, run it in a terminal
to collect history without registering a service. It lasts until stopped,
and reads the same retention and export settings as the background watcher.
