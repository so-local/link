# Upgrades

`lnk upgrade` brings every file of state Link writes to the new
release's shapes after your one yes, and it never rewrites history.

## State and History

A saved file holds either state or history, as its format's
`schema.toml` says in `holds`. State is what is so now, such as
settings, grants, approvals, the map and journals of work under way.
History is what happened, such as a conversation, a log or a ledger.
State is the default.

### Converted or Read

An upgrade converts each file of state to its format's current schema,
so no older shape outlives it and a step can be deleted at its `until`.
History is never converted. [Each line names its own schema](../files.md#saved-files) and is read
through the steps as it was written. A format of history therefore
keeps its steps for good, and only it may leave out a step's `until`.
Its declaration refuses to list files or take records.

```toml
# schema.toml
schema = 1
kind = "link conversation event"
current = 1
holds = "history"
```

## Where Files Are

A format of state lists where its files are in `[[files]]`, so an
upgrade can find them. Each entry names a location by its key in the
map, such as `lnk.secrets`, and optionally a path inside it, where a `*`
segment stands for any one name. Its `encoding` says how the file is
written, as one TOML document, one JSON object or JSON lines. A format
kept in several places lists each. The repository check
[holds every format to a place](../../../../../../../tests/repository/docs/more/developing/testing/repository-checks.md#saved-files).

```toml
# schema.toml
schema = 1
kind = "sandbox preset"
current = 1

[[files]]
location = "link.sandbox-presets"
path = "*.toml"
encoding = "toml"
```

### Inside Something Else

Some state isn't a file at a map location: an object in a store, the
manifest inside an archive, an envelope around another format's file,
a file at a path its caller names, or a file under the system's root,
which the map doesn't hold. An upgrade can't find it, so its
format declares `holds = "inside"` and lists no files. Each value is
advanced through its steps as it's read, its owner writes it back in
the current schema, and its steps are deleted at their `until`, as
other state's are.

```toml
# schema.toml
schema = 1
kind = "archive manifest"
current = 1
holds = "inside"
```

A TOML file keeps its comments, key order, line endings and permissions
through a conversion, which is the same steps engine every read uses.
The converted text is read back as its owner's type before it's
written, and a file changed meanwhile is refused.

### Modules Stopped First

Before any file changes, [`lnk upgrade`](../../../../../../../docs/more/modules/guide.md#upgrade) stops what each module runs, as
[`lnk module remove`](../../../../../../../docs/more/modules/guide.md#add-or-remove-a-module) stops it, so no program of the older release reads
or writes a file as it changes, and says what stopped for you to start
again. A module that can't stop stops the upgrade's changes, with
nothing changed.

### Writer Locks

Each file is read and written under the lock its writers hold,
`<name>.lock` beside it (`link_files::saved::lock`), which `link_files::saved::write` and
`link_files::saved::append` take for every write, so a writer waits
for the conversion and the conversion for the writer. A folder an upgrade can't read fails it,
since a file it holds would otherwise outlive the upgrade unconverted.

### Files Kept

A file holding a path that a step of the current schema refuses is kept
in its schema, never half converted. The upgrade names it with the
step's instruction and goes on with the other files. The usual case is
a record whose new owner isn't installed, so no transfer took it. The
file is converted once its record is gone, by a later [`lnk upgrade`](../../../../../../../docs/more/modules/guide.md#upgrade).

### Files It Can't Read

A file this release can't read or convert is kept as it is too, as
`unread`: one torn mid-write, one not in its kind's shape, one a stray
file a glob found, or one a newer release wrote. The plan names each
before the yes, with what's wrong by its line, never what the file
holds, and the upgrade goes on with the others, so one file never stops
it. Upgrading to an older release this way keeps every file it can't
read, and names each.

### Specifications You Wrote

A specification, which a person writes, may list its files too. An upgrade only
offers such a file for [`lnk spec convert`](../../../../../../../modules/tooling/spec/docs/config/specs/guides/kinds/README.md#convert-a-file),
never rewriting it.

## Transfers

A transfer moves records from one owner's file into another's, as when
a setting moves to the module that now owns it. The destination's
`schema.toml` declares it in `[[transfers]]`. Each names the source's
kind and the schema it is in when its records are taken, where the
source is, and each dotted path it takes with the path it becomes.

```toml
# schema.toml
schema = 1
kind = "secret settings"
current = 1

[[files]]
location = "lnk.secret-settings"
encoding = "toml"

[[transfers]]
from = "link settings"
schema = 1
source = { location = "lnk.settings", encoding = "toml" }
take = { "secret.keychain" = "keychain" }
since = "0.0.2"
until = "0.0.4"
```

A destination that takes records lists exactly one place for its file,
and each `*` of the source stands for one of the destination's. A
transfer that needs more than a move names its owner's code in
`migration`, as `secret settings_from_link settings_1`. That code is
registered in [the owner's `migrations/` folder](migrations.md) like a step's. It receives the taken records at their
destination paths and returns what to add.

### Source's Next Schema

The source's next schema refuses each path it gave up, naming its new
owner's command. A transfer runs before any conversion, so it reads the
source in the schema its records were written in. A destination that
isn't installed takes nothing, and the source keeps its records. Its
file is then [kept](#files-kept) in its schema and named, and every
other change goes on.

### Sources It May Take From

A transfer takes only from another owner's file, and only from a
location of kind `control`, since it reads the source as a file that
steers Link. Its declaration is refused otherwise.

### Journal

`link_files::upgrade` runs a transfer under the locks of the source,
the destination and the journal, `~/.lnk/safe/upgrades/transfers.toml`.
The transfer is recorded with the destination's whole new text before
anything is written. The destination is committed first and the source
second, and the entry is removed once both are done. An entry names
its files by their locations' keys and what each `*` stood for, so a
rerun finds them wherever the map has them then. The journal is a file
of state too: its format lists its place, and core converts it.

### Reruns and Refusals

A rerun after an interruption finishes the transfer from its entry and
takes nothing twice. Until the destination is written, the source is
untouched. An entry whose source or destination changed in that time is
dropped, and the records are taken again from both files as they are,
under the same locks. A destination already holding a taken path with
another value is refused before anything changes, and a rerun once
it's resolved takes the records.

### A Rerun With Nothing Planned

A rerun that plans no change still has
the programs finish a transfer the journal holds
(`link_files::upgrade::under_way`), which you agreed to when it
started.

### Source Changed Meanwhile

Once the destination is written, the source loses each taken path. A
source another writer changed since keeps the upgrade only from a path
holding another value than the one moved. That is refused, naming it,
and the rerun finishes once one of the two is kept.

## Asking the Release

Each program owns its formats' files, so [`lnk upgrade`](../../../../../../../docs/more/modules/guide.md#upgrade) asks each of the
new release's programs, core included, through the discovery request
`--link-upgrade <stage>`. A program names its formats with
`link_core::saved::own`, each as `Saved::of::<T>()`, before it
describes itself. A format shipped beside the code that reads it, for a
value of another crate's type, is `Saved::shipped::<T>(&FORMAT)`. A
format is made only when an upgrade asks, so a command's start pays
nothing for it.

### Formats Several Programs Write

A format several programs write is named by each of them, so the one
installed converts it. The plan shows each change once, and the first
program asked makes it; the others find the file already current.

### Protocol

The asking core states its upgrade protocol version in
`LNK_UPGRADE_PROTOCOL`. The program answers in the version the core
stated, or its own where that's older, so an older core reads a newer
program's answer and a newer core a rolled-back program's.

### Requests

The first
plan the core asks gives nothing on standard input; each later request
is a JSON object naming a version and a list of changes, written in the
version that program's answer stated. The program reads a request's
version before its shape, with `Protocol::read`, and refuses one it
doesn't read, older or newer.

### Versions Kept

A version that changes the answer's shape
keeps a writer for each version it still accepts, and the oldest it
accepts is declared in `upgrade/protocol.toml`. A core below that is
refused, naming the upgrade. Only a person asks without a version, and
only for a plan, which is answered in the program's own version.

### `plan`

The program lists each file it will convert, each transfer it will run
and each specification of yours it offers, changing nothing. It tries each
conversion in memory, so one that would fail is refused before your yes.
Its request names the transfers every program of the release will run,
and each source is tried without the paths they take. The core gets
those transfers by asking every program for a plan with none first.

### `transfer`

The program runs each of its transfers, through the journal, once the
release is in place and before any program converts a file.

### `convert`

The program converts each of its files of state to the current schema,
reading each back as its owner's type before writing it.

### Agreed Plan

A stage that writes is given the plan you agreed to and makes only its
changes. A transfer or conversion that differs from it, as when a file
changed after your yes, is refused, and `lnk upgrade` asks again on its
next run. Finishing a transfer the journal holds is never refused. A
program asked to write by an asker stating no version refuses.

### Order

[`lnk upgrade` downloads and checks the release](../../../../../../../docs/more/modules/guide.md#upgrade), asks for every plan,
and shows the changes once with what it installs. Once
the release is in place, every program runs its transfers, then every
program its conversions.
