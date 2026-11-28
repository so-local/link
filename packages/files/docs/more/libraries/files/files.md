# Files

> Files and Their Trusted Boundaries

`link-files` is how Link keeps files on this machine: its folders, and
reading and writing them so no other user can swap, read or change
what's in them.

## Home and Hidden

The code names Link's two roots Home, `~/Link`, and Hidden, `~/.lnk`. [Where Link Keeps Things](../../../../../../docs/more/reference/locations.md) explains what each holds, how each is split, and every location in them. The shared Rust API for both is `link_files::layout`.

### The path catalogue

[`layout.toml`](../../../../layout.toml) defines the roots and every fixed location. `Home` and `Hidden` are typed location identifiers. Their `path()` and `try_path()` methods resolve the appropriate root; `relative()` reads its registered path. Callers add only their own names and files below a location, rather than rebuilding a root path.

### Explicit roots

`Hidden::Installation.at(root)` reads the map in installation folder `root`, and
`Home::Settings.at(root)` reads this machine's map when `root` is the
Link folder it names. `at_default(root)` is the layout's default under
`root`, as a new machine has it. `Hidden::OpenClawDoctored.in_home(home)` resolves an isolated harness's state without consulting `LNK_INSTALLATION_DIR`. `Root::Home.default_in(home)` names the default Home root without reading or creating files. The catalogue refuses hidden components in every Home location.

### Trust Boundaries

Each location declares its kind as a typed `control`, `exec` or `data` boundary, the three kinds [Safe, Exec and Data](../../../../../../docs/more/reference/locations.md#safe-exec-and-data) explains. A module cannot claim the core owner `link`.

### Owner Files

`seen_specs(owner)` names the specifications an owner (`sandboxes`) has observed,
inside its own Safe. The filename is shared layout data, and each module
supplies its owner's word rather than adding a global specifications folder.

### Harness homes

Each agent's folder in `Hidden::AgentHomes` holds its harnesses' homes, one each, in the folder `layout.toml`'s `[agents] harness-homes` names. `harness_homes_at(hidden, agent)` names that folder under an explicit Hidden root. `in_harness_home(path, hidden)` says whether a path is a harness's home or inside one, an agent's own or the one its owner's agents share in `Hidden::SharedHarnesses`, never the folder of them or the rest of the agent's folder. Harness homes are data, granted separately from protected control and programs.

### Installed Programs

`exec_in(location, home)` selects an adapter's program tree. Link hands the adapter `LNK_EXEC_DIR`, first naming its private runtime staging and then a published directory under `exec/harnesses/<harness>/<agent>/<adapter hash>` (`harness_exec(harness)`). A relative or traversing handoff is refused. This value grants no access. Link supplies the sandbox's read-only grant independently.

### Publication

An installer writes its own staging while it runs. After it exits, Link copies its output through directory handles, relocates staged paths and publishes a complete tree. Copying follows no source link. An adapter version has its own tree, so publishing another leaves running agents' programs in place. A run never receives write access to installed programs.

### Memory Runtime

`MemoryService::at(folder)` names a memory service's writable runtime, generated handoff and storage sockets from the same catalogue. `in_memory_runtime(path, hidden, write)` checks a resolved grant against those areas. Only `run/` is writable. Generated `agents-<generation>.json` handoffs and `bridges/` are read-only, while agent records, generation caches and service logs remain closed. The sandbox refuses any other grant inside `Hidden::SharedMemory`.

### Root accessors

`home_dir()` names the operating system user's home, with `.` as its missing-`HOME` fallback. `try_home_dir()` requires an absolute `HOME` and resolves a linked home before use. `installation_dir()` and `try_installation_dir()` name Hidden. An absolute `LNK_INSTALLATION_DIR` selects a process's isolated Hidden root, whose map locates Home.

### Temporary security probes

[`lnk sandbox check`](../../../../../../modules/security/sandbox/docs/tooling/sandbox/guide.md#sandbox-guide) may temporarily create native dotfile probes in writable folders, including Home, to test confinement. These probes are the exception to the visibility rule and are removed after checking. See [temporary native names](../../../../../../modules/security/sandbox/docs/tooling/sandbox/security.md#temporary-native-names).

## Owners and the Map

Two declarations shape every path: whose folder a location is in, and
where it is on this machine.

### Owners

Each location also names its owner, a word of `[owners]` or `link` for
Link's own. An owner has one folder in each part it declares, named as
[Folder Names](../../../../../../docs/more/reference/locations.md#folder-names) says.

### What the Layout Refuses

`Layout::validate` refuses a location outside its owner's folder, Link's
own inside one, and an owner whose folders don't match its declared
parts, so each path has its twin in the others or is declared one-sided.
`reserved(owner)` names what the layout keeps in an owner's folder,
which none of its things may be named.

### The Map

Each location's path on a machine is [the map's](../../../../../../docs/more/reference/locations.md#moving-a-location), read by
`layout::map`. It maps a location's stable key, or `link` for the Link
folder, to its path. The map itself is at a fixed path, `Hidden::Map`,
the one location never mapped. [`lnk location list`](../../../../../../core/cli/docs/config/locations.md#list-the-locations)
and [`lnk location set`](../../../../../../core/cli/docs/config/locations.md#set-a-location) are its commands.

### Reading It Once

One command resolves dozens of paths, and each one in the Link folder asks the map where that folder is, so `map::load_cached` (which `link_in` and `try_link_dir` use) reads and checks it once, and again only when the file has changed: another file in its place, a new modification time, or a new length. A refused map isn't kept, so each asking refuses it again. `map::load` always reads the file, for a change made under the map's lock.

### Pinning

`prepare()` pins every key not yet in the map to `layout.toml`'s
default, under its lock, so a later release changing a default leaves
an existing machine as it is. `map::set` moves a location's files and
its entry with every entry inside it, or only the entry when the files
already moved.

### First Need

Folders are created on first need. Reading a path or printing a specification
creates no control roots or map. Before untrusted code starts,
`layout::prepare()` protects Hidden's Safe and installed programs, and
Home's Safe when Home exists. A missing Home stays absent until an
operation needs it, so moving the Link folder outside Link does not
recreate its old path before [`lnk location set link`](../../../../../../core/cli/docs/config/locations.md#set-a-location) selects the moved folder.

### Refusals

`map::load` refuses each entry [What the Map Refuses](../../../../../../docs/more/reference/locations.md#what-the-map-refuses)
lists, and a schema newer than this release's.

### Renamed Locations

A location's key is stable, but a release may rename one: the new name
in `layout.toml`, and a step in the map's schema,
`src/layout/map/schema.toml`, renaming its entry, its key quoted since
it holds a dot (`'locations."link.documents"' = 'locations."link.files"'`).
An older map advances through the step as it's read, its path and
comments kept, so `Key::parse` sees only current names and an old one
is refused as no location. An archive's `[map]` names locations too and
follows by the archive's own step.

### Saved Files

Every file Link saves names its schema: settings, state, grants,
approvals, the ID ledgers and the map, each at version 1 at launch. Its
owner declares it with `link_files::specification!` beside its type,
naming a `schema.toml` that holds its kind, its current schema and its
steps, the same declaration a [specification's](../../../../../spec/docs/more/libraries/types/specifications.md) owner writes.
The steps engine is `link_files::schema`, which `link-spec` re-exports.

#### Reading and Writing

`saved::read` and `read_optional` read a TOML file as control and
advance it through its steps, and `saved::write` writes it whole with
`schema` first. `parse_json` and `print_json` do the same for a JSON
object, its steps applied to its keys. `advance` gives the current text
to an owner that parses it itself, as the secret store does so its
errors never show a value. A saved file or a specification file with no
`schema` is refused, since every file Link saves names the one it was
written in, and one written by a newer release is refused, naming [`lnk upgrade`](../../../../../../docs/more/modules/guide.md#upgrade).

#### Line Ledgers

A file Link only appends to, a line at a time, names its schema on
every line: `saved::line` writes a value as one JSON line with `schema`
last, and `parse_line` reads one, advancing it, so a ledger written
across an upgrade holds lines of two schemas, each read by its own. A
line a newer release wrote is refused, naming the upgrade, never
skipped. The repository's saved check refuses a bare line appended to a
file.

#### Upgrades

Reading never rewrites a file: the release reads an old schema through
its steps in memory. A step that renames, removes or refuses a key is
declared data in the owner's `schema.toml`, never a branch in its code.
[`lnk upgrade` converts each file of state](files/upgrades.md) to the current schema after
your yes, runs the transfers that move records to their new owners, and
never rewrites history.

#### Steps That Run Code

A change no rename, removal or refusal can say is a step that names its
owner's code, kept in [the owner's `migrations/` folder](files/migrations.md).

## The Two Trees

[Where Link Keeps Things](../../../../../../docs/more/reference/locations.md#two-folders) draws the folders `Home` and
`Hidden` resolve to, and says what each owner keeps there. Link starts
fresh in this layout and reads no control from data. The rest of this
section covers what the code adds below them.

### Runtime Locations

Sockets live under `Root::Runtime`, apart from persistent state, at the paths [Runtime Sockets](../../../../../../docs/more/reference/locations.md#runtime-sockets) lists. `Runtime` identifies each module's socket locations, while `at(root)` supplies an explicit runtime root to a bridge.

#### Short Runtime Folder

A machine with neither `XDG_RUNTIME_DIR` nor `/run/user/<uid>`, such as a Mac, uses `/tmp/lnk-run-<uid>`, never a folder under `$TMPDIR`. A Unix socket's path must fit the system's `sun_path` field, which holds 104 bytes on macOS. A Mac's `$TMPDIR` (`/var/folders/<2>/<28>/T/`) takes about 50 of them, which left a shared tool's socket too long. The `[runtime-root]` table of `layout.toml` declares the parent and prefix.

#### Private in a Shared Folder

Every user can write `/tmp`, so Link makes the folder this user's alone (mode 700) before it makes anything inside it. A link or another user's folder already at that path is refused, never used.

## System Locations

`System` names root-owned settings, state and `exec` from the same
catalogue. `SystemRoot` names their parents, the folders
[Outside Your Home Folder](../../../../../../docs/more/reference/locations.md#outside-your-home-folder) lists for each
system. `path_for(SystemOs)` resolves another platform
without consulting the host. Each location declares its trust kind;
control cannot be placed in the `exec` root or code in state.

### Shared System Parents

Private directory creation below the system's settings, state and
program roots prepares the root-owned shared parent with `[system]
shared-mode`. Each parent stays searchable by separate service users,
while each service keeps its own children private. An untrusted parent
is refused before creation or permission changes. A shared root cannot
itself be made private through `create_private_dir`.

### System Sandbox Boundaries

`system_safes()` includes the settings root and every control location
in state, including systemd's private state aliases on Linux.
`System::paths_for(os)` lists a location's primary path and its aliases.
Sandboxes keep these closed even under a grant of their parent, and
installed system programs remain read-only. A run never creates or
changes the host's system roots.

#### State the catalogue doesn't name

State is control unless the catalogue declares it data. So every folder
already in a state root that no data location is in is closed too, and
a module whose state this catalogue doesn't name stays closed to runs.
The state root's data part, `System::Data`, is where an owner keeps
state a run may be granted, at the path [`/var/lib/lnk/`](../../../../../../docs/more/reference/locations.md#varliblnk) gives.

#### Places an owner declares

The catalogue names only Link's own system locations. A module that
is released apart from Link declares its own in a TOML file beside its code.
Each is a `SystemPlace`, one of the system roots (`settings`, `state` or
`exec`) and a relative path under it, refused as it's read when the path
leaves its root. `system_places` reads the owner's file into its own
type once, and `SystemPlace::paths_for(os)` gives its path and aliases as
the catalogue's do.

#### Folders an owner declares in `~/.lnk`

A module released apart from Link declares its folders in `~/.lnk` in
the same file, each a
`HiddenPlace`: its kind (`control`, `data` or `exec`) and its path in
`~/.lnk`. It's refused as it's read unless the path is in the part its
kind is in (`safe/`, `data/` or `exec/`), in a folder of its owner's
that no owner of `[owners]` has and that isn't Link's own. Its `path`,
`at(hidden)` and `in_home(home)` give its path as a `Hidden` location's
do.

#### Not on the map

It isn't on the map, which pins the catalogue's locations, as
[A Module's Own Folders](../../../../../../docs/more/reference/locations.md#a-modules-own-folders) says. The public
catalogue never names it, so a module that isn't public keeps its
folders to itself.

## Shared state locations

Link-owned persistent state lives below `.lnk` in the relevant home,
including an isolated harness home. `state::path(home, area)` selects a typed Hidden location from the same catalogue. The inventory below covers those isolated-home locations; the catalogue also holds the host's and Home's locations.

<!-- inventory:state:start -->

- [`~/.lnk/data/agents/context`](../../../../src/state.rs)
- [`~/.lnk/data/harnesses/configured`](../../../../src/state.rs)
- [`~/.lnk/data/harnesses/openclaw/doctor-release`](../../../../src/state.rs)
- [`~/.lnk/data/measures/measures.jsonl`](../../../../src/state.rs)
- [`~/.lnk/safe/envs/jobs`](../../../../src/state.rs)

<!-- inventory:state:end -->

### Portable Context and Configuration

`~/.lnk/data/agents/context` is the isolated home's portable agent context.
`~/.lnk/data/harnesses/configured` records completed harness configuration,
and `~/.lnk/data/harnesses/openclaw/doctor-release` records OpenClaw's setup marker.
These paths come from `state::Area`, so adapters use the selected home rather
than inventing another hidden directory beside `.lnk`.

### Execution Records

`~/.lnk/data/measures/measures.jsonl` receives a harness's own measurements.
`~/.lnk/safe/envs/jobs` contains job state, including bare-job workspaces.
A location's control or data kind comes from the shared layout catalogue,
and selecting an isolated home does not change that classification.

## Private folders and writes

What Link writes that may hold a secret is readable by this user alone,
in folders no other user made first.

### `create_private_dir`

Creates a folder readable only by this user (0700 on Unix), with any
missing private parents (also 0700), and takes away others' permissions from
one already there. A folder, or a link to one, that another user owns
is refused: in a shared folder such as `/tmp` they could have made it
first, to swap what's put in it. `check_own_dir` refuses a folder that
isn't this user's, itself and not through a link to another's. Shared
system roots retain their [catalogue mode](#shared-system-parents) and
are refused as the requested private directory.

### `write_private`

Writes a file readable only by this user, atomically, creating its
folder (0700) if it is missing; an existing `~/.lnk` is made private
too. The staged file is synced before renaming, then its parent is synced
before success is returned. Two writes at once each leave the file whole,
the last one's; a caller that reads, changes and writes it back holds a
lock of its own. `layout::temporary_name()` gives each atomic write a
visible sibling name, preserving same-filesystem renames even outside
Hidden.

### Abandoned Staging

Before a run, Link removes regular `lnk-tmp-*` files older than seven days
from this user's Safes, at most once per day. Both bounds are in
`layout.toml`'s `[temporary]`. It never follows a linked directory or
removes a staged file while its `write_private` transaction is active.
Other users' folders and shared writable Safes are skipped. The sweep
is best effort: a folder it can't read, or one deeper than 32 folders,
is left as it is and named in a warning, and the sweep is still recorded,
so it never stops the command that started it.

### Staging in Data

Old staging in data is only listed by [`lnk location check`](../../../../../../core/cli/docs/config/locations.md#check-for-staging-files), with `--json` for an
array of paths. It is never deleted. The scan covers the catalogue's data
roots owned by this user and skips linked directories. The core's Safe
keeps the last completed sweep in `maintenance.toml`; deleting it permits
a new scan on the next start.

### `lock_file`

Holds a lock file until it's dropped, and the next to ask waits. Its
folder is made private, and the file is opened in place, never replaced
and never through a link (`O_NOFOLLOW`), 0600, so every holder locks
the same file.

### `private_temp_dir` and `random_token`

`private_temp_dir(name)` is a new folder in the system's temporary
folder, `lnk-<name>-` and a random suffix, 0700, made here and now (as
`mkdtemp`): never one another user made first. `random_token()` is 32
random hex characters, for tokens and secrets Link makes, such as a
harness's local API key, a pairing code or a proxy's tag.

### `write_program` and `copy_program`

Write or copy a program (mode 755), never open for writing in this
process: one this process wrote itself could fail to run ("Text file
busy") while a child another thread forked still held the write open,
so a short-lived `install` writes it, from a copy this process wrote
and never runs.

## Reading Files

Trusted opens establish who could have written a file. Read receipts
identify which source bytes and returned text a reader observed.

### Reading a file whose writer matters (`fs`)

`read_trusted` reads a file once, where it really is, and no more of it
than a limit, naming that limit's owner in its error. A link is
followed once, here, and the file opened with no link left and checked
to be the one its real path names, so a folder swapped on the way opens
nothing else.

#### `Trust`

Who it must have been written by: the users who may own it, and with
`ancestors` each folder on its way, a folder its others write let by
only with its sticky bit, as `/tmp`; the mode bits none may have,
`0o022` by default; and with `no_link`, a link at its path refused
instead of followed.

#### What it refuses

It's opened without waiting, so what isn't a file (a pipe put there) is
refused, never waited on. A missing file is an error whose root cause
is that `io::Error`.

### Text Read Receipts

`reading::ReadReceipt` identifies the source snapshot and the exact text a
reader returned. Its SHA-256 digests include byte lengths. A source is
either whole or a bounded prefix, and an exclusive byte span locates the
selected source before normalization. Composable transformations identify
line normalization, lossy UTF-8 decoding, and whitespace trimming.

#### Returned Text

A presentation prefix, such as a pagination label, is counted separately.
`validate` checks its UTF-8 boundary, the selected text's digest, and the
source bounds. `covers_source` describes source coverage only. A prefix or
lossy decoding never covers the whole source, and a later consumer must
also account for any delivery limit. The receipt establishes neither the
reader's authority nor what a model received.

## Typed settings

`settings::read` reads bounded typed TOML through checked directory and
file handles. Control files refuse final links, linked data ancestors and
hard links. Only catalogued root-owned OS aliases in immutable system
parents may be traversed, with their targets checked the same way.
Missing documents use defaults only after every existing ancestor passes.
The user's home and the system roots [may themselves be reached through a
link](files/security.md#linked-roots), resolved once before the walk.

### Control Reads

`fs::read_control` applies the same policy to non-TOML control, allowing
this user or root to own it. `Trust::control` supplies that policy for
an explicit owner set. `fs::checked_parent` returns the pinned parent
for handle-relative operations. These APIs and their limits are described
in [File Security](files/security.md).

### Writer locks

`settings::read_optional` distinguishes a missing file from an existing
document. `settings::lock` gives writers a stable sidecar lock after
checking the document's trust, so atomic replacement does not let two
writers hold different locks for the same settings.

## Folders another may write (`nofollow`)

A harness's folder, an agent's Home or a skill's download, walked by
`lnk` outside the sandbox without following a link. Each name is
opened relative to its folder's handle (`Dir`) with `O_NOFOLLOW`, so a
link swapped in after a check is refused rather than followed, and a
file is read or written through the handle it was opened as.

### Traversing a private parent

`Dir::search_unlinked` opens a parent for access to its children without
following a link at the parent. On macOS it uses `O_SEARCH`, so a sandbox
can grant selected children without letting the harness list their
parent. Children are opened separately with their own access checks.

### Passing through to a folder

`Dir::root_to_pass` and `Dir::passing` walk from `/` to a folder without
reading the folders above it: each is opened only to pass through
(`O_PATH` on Linux, `O_SEARCH` on macOS), made first when asked and
missing. A sandbox that grants only the folder at the end, as Landlock
and Seatbelt do, then lets the walk through. A handle opened this way
takes the `*at` calls alone, so the folder at the end is opened with
`Dir::folder` or `Dir::create_folder` to be read, locked or synced.

### Opening a file

`Dir::open_file` refuses a link and a device without waiting on it.
Opened to write, a file with another name too (a hard link) is refused,
since what's written here would be written there, and `O_TRUNC` is
refused, since it would empty that other name before the refusal.

### Copying and removing

`Dir::remove_all` and `Dir::copy_into` go at most `DEEPEST`, 32
folders down, and refuse a folder past it. A removal first gives the
owner of each folder of this user's the right to write and search it, on
its handle, so read-only folders such as Go's module cache (`0555`) can
be emptied. It removes all it can past a name it can't, and its error
names the first left by its path. A copy says what it does
with a link: made again as the same link, left out, or refused, nothing
of it written, and reports what it copied and left out.
