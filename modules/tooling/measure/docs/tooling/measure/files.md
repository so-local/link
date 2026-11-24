# Files

The files `lnk measure` keeps, and what each one holds. It reads no
environment variables; `lnk help <command>` lists those each command
reads, as [`module.toml`](../../../module.toml) declares them. Inspect the resolved settings, then edit `~/.lnk/safe/measures/settings.toml` to override the defaults shown below.

```sh
lnk measure get
```

```toml
# ~/.lnk/safe/measures/settings.toml
every = "10s"
keep_fine = "1d"
keep_minutes = "30d"
processes = false
```

## Your settings

`~/.lnk/safe/measures/settings.toml` (600) holds your settings (`lnk measure
set`).

### Its keys

`every`, `keep_fine`, `keep_minutes` and `folders_every`, each a
duration (`10s`, `1d`); `folders_max`, a count of entries;
`processes`, whether each agent's processes are kept too; and
`[export]`, your endpoint's `url` and its `headers`.

### Link's defaults

What it leaves out is Link's: a look every `10s`, each kept `1d`, a
minute's summary kept `30d`, processes not kept.

## What the watcher keeps

Each look and each minute's summary, in `~/.lnk/safe/measures/`, each file
600.

### `fine/<yyyy-mm-dd-hh>.jsonl`

Each look in that hour (UTC): the machine and each agent, a line each
look, after a line naming the measures; `.jsonl.zst` (compressed) once
the hour is over. The format is in [measure](guide.md#where-things-live).

### `minutes/<yyyy-mm-dd>.jsonl`

A summary of each minute of that day, written as the looks are;
`.jsonl.zst` once the day is over.

### `now.json` and `day.json`

The watcher's last look, as `lnk measure --json` prints it (`--last`
prints only it); and the last day an hour at a time as it summed it up,
written each minute, which `lnk measure` and a watcher starting read on
from rather than read the day back. Both are made again from the files
above when they're missing.

## What it counts across restarts

The files that keep the watcher from losing or counting twice what it
read, in `~/.lnk/safe/measures/`, each 600.

### `tokens/<agent ID>.json`

The tokens of the agent's model answers, as its model route counted
them in the measure contract's shape, running since it was
first written, added to under a lock; the watcher reads what it grew by.
For an agent from before IDs, it's named by its name, as the keyring
names it.

### `counted.json`

What each running proxy had counted of its model's tokens at the
watcher's last look, and which agents their proxy counts, so a
restarted watcher counts none of it twice.

### `offsets.json`, `watch.log` and `watch.lock`

`offsets.json` is where the watcher stopped reading each harness's
file; `watch.log` is the watcher's log, and `watch.lock` the lock that
keeps it to one.

## In a harness's home

`.lnk/safe/measures/measures.jsonl` holds what the harness reports of itself
(`LNK_MEASURES`), until `lnk measure` reads it: emptied once every line
is read; moved aside to `measures.jsonl.1` past 1 MiB.
