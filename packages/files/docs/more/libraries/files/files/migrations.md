# Migrations

A migration is a schema step that runs its owner's Rust, for a change
to a specification or saved file that no rename, removal or refusal can say.

## Naming a Migration in a Step

A step from one schema to the next names its migration in `migration`,
by its kind and its two schemas, and does nothing else. This `disks`
kind once wrote a disk as one string, `"20G"`, and its second schema
splits it into a `[disk]` table with `size` and `unit`:

```toml
# schema.toml
schema = 1
kind = "disks"
current = 2
command = "lnk disks spec show"

[[steps]]
from = 1
since = "0.0.1"
until = "0.0.3"
migration = "disks_1_to_2"
```

## Registering the Code

The owner keeps each migration in its `migrations/` folder, one module
a step. That folder implements `schema::Migrations` for the owner's
type, listing each `Migration` by its name and function, and the
owner's `specification!` says `migrations` to load them.
`Format::parse_with` holds the steps and the registration to each other.
It refuses a misnamed migration, one registered twice or not at all, a
step that also edits keys, and a registered one no step names.

## Applying It

Reading a file applies the migration in its step's place, so a file of
any schema reaches the current type through the steps in order. The
migration takes and returns `schema::Body`, the document's keys without
`schema` and `kind`, for TOML and JSON files alike. A TOML file keeps
the comments and places of whatever the migration left unchanged. What
the code refuses is refused naming the kind and schema.

## Answering Its Old Flags

Every specification field has a flag made from its key path, such as
`--disk.size`. A rename, removal or refusal answers the old flag of a
field it retires from its own paths. A migration has no paths, so its
step lists the field flags its code retires in `flags`, each with what
to write instead:

```toml
# schema.toml
schema = 1
kind = "disks"
current = 2

[[steps]]
from = 1
since = "0.0.1"
until = "0.0.3"
migration = "disks_1_to_2"
flags = { disk = "say its size and unit apart: --disk.size 20 --disk.unit G" }
```

A flag listed there is refused with that text, as a refusal's is.
`flags` never edits a file, which only the code changes. A step without
a migration can't list any, since its own edits already answer them.

### Checking the Steps Answer Every Field

Where both shapes are declared, `Format::unanswered` checks the steps
against them, given each schema's field paths (`field_paths`, read from
its JSON Schema). Each flag a code step lists must be a field of the
schema it steps from, and each field of the oldest schema must reach a
field of the current one or be answered by a step. A field the code
takes away but no step names is reported by its path.

## Formats Without Their Code

A format exported to another program, as [`lnk spec convert`](../../../../../../../modules/tooling/spec/docs/config/specs/guides/kinds/README.md#convert-a-file) reads one,
carries the step's name but not its code. That program refuses a file
that needs the step, and names the kind's print command where its
`schema.toml` declares one (`command`, left out where no module that comes
with the kind prints it), since only the owner runs it. A file already past the step needs nothing from it.
`lnk spec convert` stays data-only by design: it never asks the owning
module to run its migration, and the owner's own commands read such a
file as it is.

### Saved Files' Code

A saved file's migration runs in its owner's program when [`lnk upgrade`](../../../../../../../docs/more/modules/guide.md#upgrade)
[converts the file](upgrades.md).

## Retiring a Migration

The step's `until` retires the migration with it. The kind's
registration test fails once the release reaches it, naming the code to
delete, and an older file is then refused as unknown.

## Keeping It Apart

Function code never names a migration or says what was migrated. The
[legacy check](../../../../../../../tests/repository/docs/more/developing/testing/repository-checks.md#code-for-older-versions)
refuses a migration's name and words outside a `migrations/` folder.
`link-files` proves the path with a `disks` kind of its own, from a
schema 1 fixture in TOML and in JSON.
