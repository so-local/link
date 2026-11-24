# Module Protocol

A module is a program installed and managed by `lnk`. The core invokes it
through this protocol and links no module implementation. Its owner keeps
`module.toml`, its Cargo manifest, help, tests and documentation together.

## Discovery

How `lnk` learns what a module is and what it documents, by asking the
module itself. Discovery and help work without installing operational
dependencies.

### `lnk-<id> --link-describe`

Prints a versioned, self-contained JSON component descriptor. Cargo
supplies its identity, executable, version, license and summary. The
descriptor supplies its kind, command routes, typed options, help,
required and recommended operations, install menu, removal metadata,
canonical documentation topics, and whether their first page opens
with a quickstart. Ambiguous executable identities are refused.

#### The description protocol

A descriptor is written in a version of the description protocol,
declared in `link-component`'s `description.toml` and read into a
[`link_version::Protocol`](../../../../../packages/version/docs/more/libraries/types/version.md#protocol-versions)
(`link_component::PROTOCOL`). The registry `lnk --link-registry`
prints, every module's descriptor, is written in it too, and both sides
state their version.

#### The asker's version

The asker states its own in `LNK_DESCRIPTION_PROTOCOL`
(`link_component::asking`). The module refuses an asker below the
oldest version it accepts, naming
[`lnk upgrade`](../../../../../docs/more/modules/guide.md#upgrade), and
`lnk` refuses one asking for its registry the same way
(`link_component::asked`). Each answers in the asker's version, its own
for an asker that states none, a person running the command by hand, or
a newer one (`Resolved::stating`, `link_component::registry_stating`).

#### The descriptor's version

The descriptor states its version in `protocol`, which the asker reads
before any other field (`Resolved::parse`,
`link_component::parse_registry`) with `Protocol::read`. A descriptor in
a version the asker doesn't read, below its oldest or newer than its
own, or stating none, is refused by the module's name, as `lnk-notes
speaks the description protocol 1, below 2, the oldest this release
accepts: upgrade it with lnk upgrade`. [A raised `oldest` names its
bridge release](../../../../../packages/version/docs/more/libraries/types/version.md#raising-the-oldest), which the refusal names.

#### What a refusal names

A core and a module of one release speak one version, so the refusal
names a module left behind by an interrupted upgrade, or one copied in
from another release. `lnk` shows a module's own refusal as the reason
it gave no description.

### Built from the same sources

A release build from source also says, in `source`, the commit of the
checkout it was built from, which its build script asks Git for and
gives the compiler in `LNK_BUILT_FROM`. Two programs built at different
commits both report the same version, such as `0.14.0`, so the version
alone can't show that they were built apart. A release build, whose
version is exact, a build outside a Git checkout and a debug build say
none. A debug build's programs are built together by
`cargo build --workspace --bins`, and a commit in them would rebuild
every component at every commit.

#### Modules built apart

Before a program runs a module it finds (`link_core::find_module`), it
compares the module's `source` with its own. A module built from
another commit is refused by name, with its path, both commits and the
commands that build every program from the checkout together: `cargo
build --workspace --bins`, or `lnk up --source <checkout>`. `lnk`
refuses a command of it (`link_core::locate`), and another program
treats it as missing and says why once. Only two known and different
commits are refused, and a script is never asked.

#### Answerers check themselves

A module answering a question checks for itself, so the asker runs
nothing more. The asker hands its commit in `LNK_ASKER_BUILT_FROM`, and
the answerer refuses the question by its own name when its commit
differs (`link_core::source::answering`, read by `Question::input`).
The asker's error for that module ends with the refusal.

#### Modules listed and asked

Asking costs a run of the module, so only a program about to run one,
or a person reading about it, asks. [`lnk mod list --json`](../../../../../docs/more/modules/guide.md#get-help), the
listing every question reads, says whether each module's program is
there (`link_core::module_present`) without asking. A program asking a
question runs only the modules that answer it, each of which checks its
own commit. The table `lnk mod list` prints, and [`lnk mod show`](../../../../../docs/more/modules/guide.md#inspect-a-module), are
for a person, so they ask and show one built apart as not installed,
said aside.

#### When it rebuilds

Each module's build script runs again when the checkout's commit moves,
so a build after a commit rebuilds every module package, not their
libraries. Building one package after a commit leaves the others built
apart from it, and they're refused until they're built again.

### A command's help

A command's actions own its argument and flag descriptions. Clap supplies the standard help and version descriptions. `lnk variable list
<command>` lists its declared environment inputs separately, including
option-backed inputs and those of the program and ancestor commands.
The list shows names and descriptions without reading current values.
Menus, including the core's fallback for a missing module, omit hidden
commands and their descendants. An exact command path can still answer help.

### `lnk-<id> --link-documentation`

Prints its embedded documentation pages. Each page has the same
canonical path as its website page. Documentation and the runtime
descriptor are embedded at build time; test providers, source pins and
build inputs remain build metadata.

## Adding and removing modules

How `lnk` lists, adds and requires modules, and what it does for one
that isn't installed.

### `lnk mod`

[`lnk mod list --json`](../../../../../docs/more/modules/guide.md#get-help) prints `link_core::modules::Listed` entries:
`{"name", "installed", "kind"?, "description"?, "source"?, "key"?, "answers"?}`.
A module is `installed` when its program is beside `lnk` or on the `PATH`, and
listing runs none of them, since every question asks this list.
Whether a module built from source matches `lnk` is checked as it's run.
[`lnk mod add`](../../../../../docs/more/modules/guide.md#add-or-remove-a-module) asks a narrower question, `link_core::installed_module`:
whether the program is beside `lnk`, where it installs. One only on the
`PATH` is listed, and `add` installs `lnk`'s own beside it.

#### Component Kinds

The build's declarations supply `kind` and `description`. Callers use
the declared harness kind to discover, install, and describe additional
harnesses without keeping another list of their names.

#### A Module from Outside Link

An outside
module includes its repository and pinned signer. `lnk mod add
<url> --yes --signer <key>` installs its signed release, trusting only
that key.

### A missing module

A missing module returns exit status 4
(`link_core::modules::NOT_INSTALLED`) and explains how to install it.

### Requirements and recommendations

Required operations propagate the called command's typed option
bindings and platform predicates. Recommendations enter the closure
only when selected. A missing applicable requirement blocks the
operation.

## Questions

A module learns what the modules built on it have without naming them:
it asks a question, and each installed module that answers it does.

### A question

A question is `link_core::answers::Question<I, O>`, named
`<module>.<what>` for the module that asks it, and defined in that
module's crate or contract, so only a module built on it can depend on
its types. It is made with [its protocol's declaration](../../../../../packages/version/docs/more/libraries/types/version.md#protocol-versions),
`Question::new("storage.folders", include_str!("folders.toml"))`: a TOML
file beside the question, read into a `link_version::Protocol`.

### Answering

A module lists the questions it answers in its `module.toml`, as
`answers = ["storage.folders", "storage.owner"]` does, and answers them
all through one hidden flag at its program's root, `lnk-<module>
--link-answer <question>`, which its generated parser has. [The asker
runs that program directly](../../../../../packages/component/docs/more/libraries/types/component-metadata.md#question-answers), the `lnk-<module>` beside the `lnk` it runs,
else the first on the `PATH`, where `lnk` finds a module too
(`link_core::module_program`), never through `lnk`.
`Question::input` reads what the answering command was asked and
`Question::answer` writes its answer.

### The wire

Each side states its version of the question's protocol and checks the
other's. The asker writes one JSON line to the answer's stdin, its
version and the question, `{"protocol": 2, "question": "agent:main"}`
for `secret.grantee`. The answer writes one back to its stdout, its
version and the answer, `{"protocol": 2, "answer": {"here": {"workload":
"agent:a1b2c3d4e5f6", "principal": "service:s1s2s3s4s5s6"}}}`.

### Older peers

A peer below the oldest version a side accepts is refused by name, as
`the agent module speaks the secret grantee answers protocol 1, below 2,
the oldest this release accepts: upgrade it with lnk upgrade`. A bare
question or answer, out of its envelope, isn't read.

### `Question::ask`

`Question::ask` runs `lnk-<module> --link-answer <question>` for each installed
module `lnk mod list --json` lists as answering, and reads each
answer. With none installed there is no answer, so a module above that
isn't there has nothing to say. The layers check refuses an answer to a
question of a layer the answering module isn't built on.

### A failed answer

`Question::ask` keeps what each answering command says on stderr. When
one exits unsuccessfully, its error ends with the last non-empty lines
it said, cleaned of control and invisible characters and bounded as
`answers/said.toml` sets, as `the agent module
didn't answer storage.folders: Error: settings.toml is missing`. The
asker decides whether one failed answer stops it. `Answerer::reply`
keeps them too when its caller passes `Stdio::piped()` for stderr.

### Shared discovery

A process finds its answerers once. `link_core::modules::list` keeps
the first listing it gets, so every later question shares it, and
forgets it once `link_core::modules::ensure` installs a module.
`Question::answerers_in` selects typed answerers from a listing the
caller already holds, and `Answerer::reply` keeps the envelopes above.

#### Handed by `lnk`

Running `lnk <group>` replaces the `lnk` process with the module's
program, so the module keeps `lnk`'s process ID. Before that, `lnk`
writes its listing into `LNK_MODULES`, marked with that ID
(`link_core::modules::Handed`). The module reads it in place of
starting another `lnk mod list --json`. A program the module starts
inherits the variable with another ID, so it asks `lnk` for itself.

### Exchanges of their own

`Answerer::command` gives the answering command itself, `lnk-<module>
--link-answer <question>`, for a caller that exchanges with it in a protocol of
its own, such as one bounded by a deadline, as Box's machine question
is. That caller writes and reads the same
envelopes (`Question::asked`, `Question::answer_of`) around its own
messages.

## Uninstalling (`lnk-<module> uninstall`)

`uninstall --parts --json` prints `link_core::uninstall::Kept`:

<!-- json: link_core::uninstall::Kept -->
```json
{"parts": [{"name": "example", "about": "saved settings", "paths": [], "keys": [], "first": "", "secrets": [], "credentials": []}], "stops": "", "stays": ""}
```

### Each part

Each part declares absolute paths within the user's settings or data
folders and Keychain accounts, exact or a prefix ending in `/`. `first`
explains how to keep a copy. `secrets` selects paths containing private
state; omission means all paths. `credentials` contains `Held` entries
with `what`, optional `of`, `kept` and `may`, never secret values.
`stops` explains what stops, and `stays` what remains elsewhere.

### `lnk uninstall --parts --json`

Returns installed modules' declarations without deleting anything.
Only a module whose `module.toml` has a `[removal]` table is asked, so
listing what Link keeps starts no module that keeps nothing. Its
`parts` name every part it can report, even none, as `parts = []` does
for a module that only stops services. A test runs each built module
and holds the declaration to its answer. Interactive removal merges
parts with one name; `rest` is data no module claims. A module that
does not answer has no declared state or services.

### `uninstall [--delete <part>]...`

Stops the module's services and removes only selected parts, using
`link_core::uninstall::run`. It returns 0 when complete. Removing a
module without selected parts stops its services and retains its data.
A failed removal retains the executable and reports its stderr.

## Signed release advisories

`Release::advisories` reads `ADVISORIES.toml` only through the verified
checksum list. `Advisories` holds entries with unique `id`, `affects`
versions from inclusive `from` to exclusive `before`, `fixed` version,
`critical` or `recommended` level, and a plain summary. Unknown fields,
invalid ranges and duplicate identities are refused. Releases without
this asset have no advisory ledger. Failed checks retain known guidance.

## Shared libraries

Modules import neutral libraries directly: [specifications](../../../../../packages/spec/docs/more/libraries/types/specifications.md),
[downloaded artifacts](../../../../../packages/artifact/docs/more/libraries/files/artifact.md)
and [application settings](../../../../../packages/settings/docs/more/libraries/files/settings.md).
The module protocol holds command discovery, lifecycle and release behavior.
