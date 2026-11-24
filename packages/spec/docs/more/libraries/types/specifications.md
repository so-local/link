# Specifications

`link-spec` [reads specifications strictly](../../../../../../docs/more/developing/modules/specs.md), as typed TOML, and the paths in
them, written so a specification means the same on any machine. An unknown key
is refused, never ignored.

## `parse`

Parses a specification into its type, refusing unknown fields and empty tables.
An empty table is named with its line: `line 3: [network] is empty:
give it its key, or leave it out`.

## `path`

A path as a specification writes it, made absolute: `~/` is the home folder,
and anything else relative is from the specification's folder. `..` is refused,
so a specification saved anywhere can't climb out of where it says: write it
from `~/` or in full.

## `print`

A specification as TOML, by the serializer, never by hand, so a name from
elsewhere can't add a line.

## `duration_ms`

A duration as milliseconds: a number and a unit, `ms`, `s`, `m`, `h`
or `d` (`"10m"`). Anything else isn't a duration, and says so.

## Specification Shape

A specification kind's shape is the list of tables and keys a file of that kind
may hold. It is declared once, in its module's `types.toml`, as a JSON
Schema (the standard vocabulary editors use to describe a JSON or TOML
document). The Rust type, its reader, its field flags, its help, and its
reference page are generated from that shape.

### Declaring the Shape

A specification kind is a shape marked `x-link-spec` with its kind's name. Each
field's `description` is its help and its reference entry, and its
`examples` are values a file may hold there. [A field holding a path](#paths) is
marked `x-link-path`, so it resolves from the file's folder in a file
and from the current folder in a flag.

```toml
# types.toml
schema = 1

[spec]
type = "object"
additionalProperties = false
x-link-spec = { kind = "example" }

[spec.properties.timeout]
type = "string"
format = "duration"
description = "How long a run may take"
```

The module's build [implements `Specification` for the generated type](../../../../../actions/build/docs/more/developing/modules/catalog/generation.md#specification-kinds),
naming the kind's `schema.toml`, gives it the catalog's JSON Schema
for that shape, and implements `Kind` with the type's validation and
the declaration's check hooks.
The reader and the shape are then one declaration, so they can't
disagree.

### Kinds Written in Rust

`link_spec::specification!(Spec, include_str!("schema.toml"))`
registers a kind whose type is still written in Rust, deriving or
writing `schemars::JsonSchema`. The repository's shapes check allows
that only in the few files it holds, each until its module is declared.
The macro adds a test to the registering crate that holds such a type's
reader to its declared shape, as the sections below describe.

### Checking the Shape

The macro also adds a test to the registering crate that calls
`check_shape`. It compares the declared shape with the tables the type's
reader actually takes, found independently by `link_spec::probe`. The
probe drives the type's serde `Deserialize` implementation down one
dotted path at a time and records the keys it expects, without building
a value. Every table either side has must match by path and keys.

#### Values Read by Hand

Where the reader is written by hand, the probe can't see inside, so the
schema there must give examples. The reader must take each one at that
path, and every key the schema declares inside must appear in one of
them, so a key the reader stopped taking fails its example. A failure
names the kind, the path and what disagrees.

### Checking Real Readers

Registration also calls `link_spec::check_roundtrip<T: Kind>()`. It produces
Every Option from the declared shape, reads it through the kind's actual
reader and `Kind::check`, then serializes the typed value to TOML and reads
it again. A schema that accepts an example is not sufficient if the real
reader or its semantic validation refuses that value.

#### Retained Primitive Type Refusals

Registration calls `link_spec::check_type_refusals<T: Kind>()` with a valid Every Option baseline and equal typed file and flag values. It changes one retained string, boolean, integer or number field to a contrasting TOML kind and requires the owning reader to refuse it.

- `type_refusals<T>()` records each concrete witness and exclusion; open or mixed types and containers receive no primitive witness.
- Primitive list items and fields absent from typed serialization or emitted flags receive no type proof.
- The sampled input and wrong kind supply no omitted-default, unsampled-arm, custom-validation or runtime-effect proof.

#### Structural Field Type Refusals

Registration calls `link_spec::check_structural_types<T: Kind>()`. The reader probe records actual typed requests and their optional and container routes. The owning field's `DeserializeSeed` then receives a declared control and a contrasting value through TOML's real deserializer. The control must be accepted before the contrasting literal's refusal supplies a structural field-reader witness.

- `structural_types<T>()` records exact controls, counterexamples and exclusions; requests alone prove no refusal.
- Opaque or unreached paths, unassociated enum routes and fields without an accepted declared control receive no proof. The example reader's scalar-to-list convenience remains separate.
- Only the leaf result supplies proof; completion of the surrounding deserializer, `Kind::check` and default behavior supply none.

#### Retained Container Type Refusals

Registration also calls `link_spec::check_container_type_refusals<T: Kind>()`. After the same valid file and flag baseline, it changes each actual array or object field retained by typed serialization to a boolean and requires the owning file reader to refuse it. `container_type_refusals<T>()` records concrete fields and exclusions. A whole container needs no separate emitted flag for this file-input refusal; open or mixed kinds, list elements without their own declared field, and containers absent from typed serialization receive no witness.

#### Required Scalar Omissions

Registration also calls `link_spec::check_required_refusals<T: Kind>()`. It removes one actual retained primitive field whose single object shape independently declares that key required, and requires the owning file reader to refuse the omission. `required_refusals<T>()` reports concrete omissions and exclusions; optional or defaulted keys, composed object obligations and unsampled fields supply no required-field proof. This does not infer the value of a default from the reader.

#### Retained Scalar Choices

Registration calls `link_spec::check_scalar_choices<T: Kind>()` on retained active fields with one unconstrained finite string domain or boolean kind. It reads each declared literal through the owning reader, requires the typed value to preserve it, and compares the actual emitted flags with that typed configuration.

- A string outside the closed domain must be refused.
- `scalar_choices<T>()` records exact values and exclusions; open or composed alternatives, additional constraints, and values omitted by serialization or flag emission receive no choice witness.
- Choices are checked in the sampled surrounding configuration; they supply no omitted-default, unsampled-arm or runtime-effect proof.

#### Finite Choice Precedence

Where a retained scalar choice has a different declared literal, the same guard reads that other literal as a valid file first. If the two typed configurations differ, the first choice's real emitted flags must override that conflicting file and reproduce its full typed configuration, including unrelated fields. Each `ScalarChoice` witness records the conflicting file literal in `precedence`; a single-valued domain or two spellings with the same typed result supplies no precedence proof.

#### Maintained Example Contexts

Registration calls `link_spec::check_example_readers<T: Kind>()`. It inserts literal maintained schema examples at existing Every Option pointers and requires each accepted complete file to preserve its typed configuration through actual flag replay.

- Retained type and scalar-choice checks apply only within the inserted subtree.
- `example_readers<T>()` records qualified input, typed and flag tables with exact witnesses; fragments refused by the owning file reader or `Kind::check` are explicit context exclusions.
- A fragment alone supplies no complete-reader proof, and an accepted file whose flag replay fails remains a check failure.

#### File and Flag Equivalence

The same check replays the effective fields as generated flags through
`split` and the kind's reader. Boolean values inside opaque types, nested
lists, and composed paths retain their file semantics. Printed output must
contain only specification fields, so a runtime argument emitted by a serializer
fails registration even when deserialization ignores it.

## Field Flags

Every field of a registered kind has a command-line flag, generated from
its declared shape, so a field added to the type has its flag in the
same build. A flag is the field's TOML key path as the file writes it,
`--model.thinking`, and a top-level key's is `--spec.<key>`, so every
field flag holds a dot and no runtime flag does. The format envelope's `kind` and
`schema` take no flag.

### Splitting a Command Line

`split` takes every field flag out of a command line before clap reads
the rest, checking its path against the shape of the specification the command
binds. A path the shape lacks is refused with the nearest flag there is,
and a command binding no specification refuses every field flag. Nothing after
`--` is a flag. The flags form one TOML table, `FieldFlags`, and
command-line defaults never enter it.

#### Values

A value is the TOML value the file would hold, read as text where the
field takes a string, so `--model.thinking high` needs no quotes. A
switch given alone is `true`. Repeating a list's flag builds the list,
and a whole array such as `--files.write=[]` replaces it. A list of
tables takes whole inline-table elements, since its elements have no
names. A key the writer chooses is part of the path, quoted as TOML
quotes it: `--tool."social.post".limit 4`.

#### Paths

A field whose schema has `"format": "path"`, through
`#[schemars(extend("format" = "path"))]`, resolves as `path` does, a
flag's from the current folder and a file's from the file's folder,
before the flags are laid over the file.

#### Bundle Paths

A host-side file constrained to the specification bundle instead uses
`link_spec::CONFINED_PATH_FORMAT` (`"confined-path"`). File values resolve
from the source folder and flag values from the invocation folder; paths
outside the bundle are refused. The reader rebases accepted paths to safe
child-relative names before `Kind::check`. Guest paths such as a stage
workdir stay opaque strings and receive neither host-path treatment.

#### Text With a Source Folder

For text with a known source folder, `read_text_at<T>(text, absolute_source_dir,
flags)` supplies that context. `read_text_with` has no source context;
callers reading captured or named bundle contents should use the contextual
entry point rather than resolving paths themselves.

### Reading Flags Over a File

`read_with` reads the file in the current schema, applying its steps,
lays the flags over it key by key, and reads the result once through the
kind's strict reader. File and flags therefore share validation and
defaults, and a bad value is refused with the reader's message from
either input. A table merges into a table, and any other value,
including a list, replaces the file's.

### Refused Input

What `read_with`, `read_spec`, `over` and `show` refuse is marked `link_format::Refused`, so its report
is a usage error, exit code 2, with no [`lnk issue create`](../../../../../../docs/more/modules/guide.md#get-help) footer: the person
fixes their file or flag, and Link has nothing to report.

### Help and Inspection

`help` lists the field flags with how each value is written and the
first paragraph of its description, a free key as `--variables.<name>`.
`with_field_help` adds that list after a command's own flags.
`SpecCommand` is the `spec show` command each kind's group carries, and
`show` prints the effective specification of a thing, through the owner's hook, a
`--spec` file or flags alone, in TOML with its `schema` line or as JSON.
Showing never writes anything.

### Holding Flags to the Shape

The registration test also calls `fields::uncovered`, which fails for a
declared key no flag reaches, and `fields::check_spelling`, which
refuses a top-level field named `spec`, `kind` or `schema`.

## One Entry

`run` is the one entry a program's command line goes through. A program
supplies a `Program`: what Link answers before any parser, its look, its
help, its history and the specification kind each command binds. `link-core`'s
`Help` is every module's. `run` takes the bound specification's field flags,
parses the rest with the command's clap type, and chooses JSON output
from the parsed command (`asks_json`), never from a scan of the
arguments. `try_run` returns its refusal, `Refused`, instead of exiting.

### Retired Spellings at the Entry

`run` refuses a retired command or flag before anything parses the
line, as a report with its replacement as the next step, as JSON where
the line asks for `--json`. A command is found by its words, so a
global flag given before them, such as `-a main` before the [agent](../../../../../../modules/agents/agent/docs/agents/README.md#agents)'s `allow`, and a flag it
doesn't know after them never hide it. A retired variable that is set
is a warning naming it, never its value, and the command runs.

### Writing a Command Line

`argv` (`link_spec::argv`) writes a command's typed arguments back as the command line that
makes them, for a program that runs another: each flag by its long
name, a `true` boolean as the bare flag, a list as one flag a value,
operands in order and a trailing one after `--`. What's unset, `false`
or empty is left out, so the line parses back to the same value. A
caller builds the line from the type the other program parses, never by
spelling its flags.

### Shared Flags

`shared` holds one type per shared flag group, `Output`, `Quiet`,
`Confirm`, `Relay`, `Request` and `Actions`, each in the clap group
`shared.<group>` and serialized as its one value, so a command's file
form keeps the key `json` or `yes`.

## Reference Pages

`reference` renders the parts of a kind's reference page that come from
its shape, which `link-actions-surfaces` writes from the kind's
declaration: Every Option (`every_option_of`), and the section of each
top-level key (`key_sections`), whose H2 the page writes, with an H3
per field below. Each field's entry opens with a quoted line of its
type, values, default and flag, and its description follows. A kind
another holds whole (`Composed`) is linked to its own page, titled by
`title` (such as Agent Specification), never inlined.

### Every Option and Descriptions

`every_option` gives every field an example value: a schema's own
example or default, else its first value, else a value of its type,
the first variant and one free key, `name`. `undocumented` names every
field without a description, which a generated page refuses.

## Versioned documents

A published specification implements `Specification`, which its module's build
generates, naming its owning schema file. The declaration gives its kind, current
schema and print command. A specification file names its `kind` beside its
`schema`, and a reader refuses a file naming another kind while taking
one that leaves it out. Its `schema` it never leaves out: a specification file
naming none is refused, telling you to add the schema it was written in,
since no release can tell which one an unmarked file means. A specification made
from flags alone is in the current schema.

### Reading Older Formats

`parse_spec` reads the positive integer `schema`, applies every
declared step to the current format, then reads the current type
strictly. Future formats are
refused with an instruction to run [`lnk upgrade`](../../../../../../docs/more/modules/guide.md#upgrade). The steps engine is
`link_files::schema`, [shared with every file Link saves](../../../../../files/docs/more/libraries/files/files.md#saved-files), and
`link_spec::schema` re-exports it.
Printing writes `kind` and `schema` at the top, while a saved file that
isn't a specification keeps its numeric marker alone.

### Transformation steps

A `schema.toml` opens with `schema = 1`, the version of the declaration
format itself, which a reader of another version refuses. Its steps are
a `[[steps]]` list.

#### Steps

Each step declares the source schema in `from` and advances one number.
`rename` maps dotted table paths to their new paths. `remove` maps paths
to explanations. `refuse` maps paths to instructions for the user,
including a value whose meaning changed. A `rewrite` step is refused.
Keep every published step, with no gaps, until its `until`. A transformation that widens access must refuse
instead of guessing consent. Colliding names are refused, and strict
validation still rejects unknown fields after transformation.

#### Quoted Keys

A key that a bare TOML key can't spell, as one holding a dot, is quoted
in a path, and only such a key, so each field has one path:
`locations."lnk.migration-journal"` is the key `lnk.migration-journal` in the table
`locations`, as [`schema::path`](../../../../../files/src/schema/path.rs) reads it.
A path quoting a key that could be bare, with a broken quote, or with a
`\` inside quotes, which TOML would read as an escape, is refused as the
declaration is read. A `[before]` key is read the same way.

#### Migrations

A change no edit can say is a step that names its owner's code in
`migration`, which only [its `migrations/` folder](../../../../../files/docs/more/libraries/files/files/migrations.md) holds. A kind with one
registers with `specification!(Type, source, migrations)`.

#### Retirement

Every step names the release that brought it, `since`, and the release
that deletes it, `until`, after `since`. `check_format`, which a kind's
registration test runs, fails once the current release reaches a step's
`until`, naming any code the step runs to delete with it, and it holds
every kind's name to one lowercase word.

### Retired flags

A field's flag is its key path, so a step that renames, removes or
refuses a field retires that flag with it. When a field flag names a
path the shape lacks, `split` looks for a step that retired it: a rename
names the current flag, followed through every later step, and a
removal or refusal gives the step's reason. `Binding` carries a kind's
shape and its format for this. At the [one entry](#one-entry), such a
flag is refused as a report with the flag that replaced it as the next
step.

### Shapes before 0.1

Link 0.1 starts every schema again at 1, and no specification kind Link includes
lists anything under `[before]`. Since 0.1 keeps no install made
earlier, a file an earlier release wrote is read by its schema number
like any other: a number above the current one is refused as newer, and
a key the shape lacks as unknown.

#### Listing `[before]`

The table stays for an owner that must name what came before 0.1.
Under `[before]`, `schemas` maps each old number to what to write
instead, and `keys` maps each dotted path to what replaced it, a `*`
matching any one key or list entry but never first. A file naming a
listed schema, or holding a listed key once its steps have run, is
refused by name with that text.

#### Retiring `[before]`

Like a step, `[before]` names its `since` and `until`: `check_format`
fails a kind's test once the release reaches its `until`, and
`check_before` fails one whose `[before.keys]` names a key the current
shape declares.

### Exported kinds

`Exported` is a kind as the module owning it exports it in its
description: its name, current schema, print command, steps and shape.
A program names the kinds it owns in `describe_component!`, each
exported only when first asked for. Every installed module's kinds are what [`lnk spec`](../../../../../../modules/tooling/spec/docs/tooling/spec/README.md#specification)
lists, shows and converts.

### Converting a file

`convert` rewrites a specification file in its kind's current schema, by the
exported steps alone. A step that names its owner's code is refused,
naming the kind's print command, since only the owner runs that code. It names the kind at the file's top, keeps the
file's comments, key order and permissions, checks the result against
the kind's shape with `fields::check_value`, and refuses a source that
changed while it was read. It is the only code that rewrites a specification
file. `fields::Show` is every kind's `spec show`, and unrelated strict
TOML reads use `read` and `parse`.

## History

A module's `history.toml` records the commands, runtime flags and
`LNK_` variables it retired, in the one schema `link_spec::history`
owns. Each `[[gone]]` entry names the old spelling in `was`, what
replaces it in `now` or why in `why`, the release it's gone `since` and
the release that deletes the entry, `until`. The core's history also
keeps `[[renamed]]` modules and `[[removed]]` ones, each removal with
its `why`. Link 0.1 starts every history empty, so an
entry records only what a release after 0.1 retired, as in this
example.

```toml
# history.toml
[[gone]]
was = "agent show --settings"
now = "lnk agent spec show"
since = "0.3.0"
until = "0.6.0"
```

### Spelling an Entry

An old command or flag is spelled as a person types it after `lnk`, its
module's namespace first, as `agent show --settings` is, even a command
a program is only run with directly. The parser puts the namespace back
before it looks, so the module's build refuses an entry without it.

### Tools in History

A tool a model calls is a command of its tool module (`memory-tool`), and a
call a model makes is spelled by the name it calls, `memory-tool
search` for `memory__search`. A renamed call is a `[[gone]]` entry of
its module, and a renamed tool is its module's `[[renamed]]` entry in
the core's history.

### Replacements Anywhere in the Release

`validate` holds an entry against two surfaces. No entry may still be a
command, tool call or flag of its own module. Its `now` may name any
command of the release's whole surface, every module's and the core's,
so a command moved to another module names its new home. The docs
checks hold every module's history to that whole surface.

### Answering an Old Spelling

A module's build embeds its history in its description, and its parser
answers a command line it can't parse from that history: an old command
or flag is told its replacement and never runs. Matching reads syntax
alone, never a flag's value or anything after `--`. A renamed module's
old name is answered with the name it has today, through every rename
since, and a removed module's name with its `why`. `check_retirement`
names every entry the current release has reached the `until` of, which
a test turns into a failure, so history never accumulates.

### Names That Follow History

A specification can name a module, a command or a tool as a value, as the keys
of a `[tool.<name>]` table do. Its declaration [marks such a string](../../../../../actions/docs/more/developing/modules/catalog/types.md), or a
map's keys, with `x-link-ref`.
`follow::sites` finds each marked place in a kind's shape, and every
read of the kind follows them after its steps.

#### How the Places Are Found

A definition naming itself, as a sandbox section does for each
platform's overlay (`tools.linux`), is walked once more and never a
third time, and each kind's places are found on its first read and
kept. An old name becomes the one it has today, and a removed one is
refused by name. The file's own text is never rewritten.

#### Where the Release's History Comes From

The release's history is the core's, its modules renamed and removed,
with each module's retired spellings. The core answers it to
`--link-history`, and a program says how to ask at its start
(`Program::release`). Nothing is asked until a name is read. A program
that says none reads names as written. One whose core doesn't answer
refuses each name that follows, so none is read as written in a release
that renamed it. A specification the release wrote itself, such as a module's
built-in defaults, is read by `parse_current` as written, never asking.

#### Recorded Calls

`follow::replayed` names a recorded call (`memory__search`) as its tool
and command are today, for a program replaying a conversation to its
model or reading the tools a conversation is narrowed to. It asks for
the history only for a name holding `__`, as a call of Link's tools
does, and never refuses one: a call since removed keeps its name, and
so does every call where the history can't be read, which its tool then
answers as a name it doesn't know, never as another.
