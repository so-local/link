# Files and Variables

The files and environment variables every `lnk` program reads, and those of the core and its installer.

## Every `lnk` program

Each program reads its declared environment inputs. Command help covers
arguments and flags. Use `lnk variable list` to browse command variable names,
the commands that declare them and their descriptions. The list comes
from the core and this build's module registry and never reads current values.
The [environment index](../../../../../docs/more/reference/environment.md)
also covers libraries, internal overrides and tests.

### List a command's variables

Pass command words to narrow the list. Program-wide and ancestor command
inputs remain included, alongside the selected command's own declarations.
An unknown command is refused. Use `--json` for an array of entries with
`name`, `command` and `about` fields.

```sh
lnk variable list agent start
```

### Module Output Flags

For a module, put output flags after its command, as in
[`lnk agent list --json`](../../../../../modules/agents/agent/docs/agents/start/create.md#run-more-than-one-agent); the core refuses a leading `--json` instead of
silently dropping it when it hands the invocation to the module.

### `~/.lnk/safe/releases/update.json` (600)

The latest release's version as [`lnk upgrade --check`](../../../../../docs/more/modules/guide.md#upgrade) last found it, when it
checked, advisories affecting this version, and when each notice last
appeared. Deleted, the next interactive command schedules a fresh check.

### `~/.lnk/safe/modules/offers.toml` (600)

What goes with a module that you refused (`refused`) or were told of without a
terminal (`said`): neither is offered again. Deleted, each is offered again.

## The core (`lnk`)

The core separates the machine's Home locator from the settings a person edits. Both files are readable only by you; a moved Home keeps its settings and marker together.

### `~/.lnk/safe/settings.toml` (600)

This machine's settings: `id`, the identity in the Link folder's marker,
and `noted`, programs beside `lnk` that `lnk upgrade` has said once how to
keep updated. The folder's location is held in the shared path map,
`~/.lnk/safe/map.toml`, rather than in this settings document.

### `~/Link/Safe/settings.toml` (600)

Your output settings and the modules you add from outside Link live in Home. The machine's locator and installation notes remain in Hidden; editing a setting never changes Home's identity.

### Staging Files

Preparing a run cleans expired atomic-write staging from your Safes. `lnk folder check`
lists old staging in your data without deleting it, and `--json` returns
an array of paths. See [Abandoned Staging](../../../../../packages/files/docs/more/libraries/files/files.md#abandoned-staging)
for the age, cadence and scan boundaries.

```sh
lnk folder check
```

### `[modules.<name>]`

Each module added from outside Link, with [`lnk mod add --url <url>`](../../../../../docs/more/modules/guide.md#add-or-remove-a-module), in Home's `Safe/settings.toml`: its `source`, the repository, and the `signer` its releases must be signed with (`ssh-ed25519 <base64>`).

## `install.sh`

The installer reads its environment, as [its own header](../../../install.sh) lists it.

### `--source`

From a checkout, `sh core/cli/install.sh --source` installs only the CLI
through Cargo. An optional path selects another workspace. Without
a path it uses the workspace containing the script. This mode needs Rust
and Cargo, honors Cargo's installation root and uses the workspace's
`target` folder. It bypasses the release settings below.

### Source module installation

The installed CLI accepts repeated [`lnk up --source <path>`](../../../../../docs/more/modules/install.md#pick-your-modules) arguments,
including workspaces containing only modules. It excludes core and
fixture packages. Without selection flags it installs all supported
modules where the running `lnk` is. Run from a Cargo target folder, it
builds them into that folder with its profile. Run from a Cargo
installation's `bin`, it installs them with `--root` into that
installation, building in the first workspace's target directory.
Anywhere else it refuses before changing anything. It installs only
module binary targets and does not restart agents.

### Source module selection

Separate names with commas or repeat `--module <name>` on `lnk up --source <path>` to select executable
names without `lnk-`. Every workspace and module name is validated before
installation. Duplicate roots, conflicting module names, unknown selections
and explicitly selected unsupported platforms are refused.

### Source module freshness

With `lnk up --source <path> --stale`, only installed modules are checked.
Beside a target-folder `lnk`, Cargo rebuilds each selected binary in place.
Beside an installed `lnk`, Cargo builds it in release mode with the locked
dependencies. SHA-256 comparison of that artifact and the executable beside
`lnk` skips identical binaries. Changed binaries are reinstalled through
Cargo with `--root` pointing at `lnk`'s installation. No separate
freshness database is kept. A failed build preserves that module's installation.

### Source package platforms

A package can restrict source installation with `os` and `arch` string
arrays in `[package.metadata.link.source]` in its Cargo manifest. Values
use Rust host names such as `linux`, `macos`, `x86_64` and `aarch64`.
An absent or empty array permits all values for that field. The installer
checks both fields before invoking Cargo and names skipped packages.
These declarations select source packages, not runtime qualification.

### `LNK_INSTALL_DIR`

Where to put `lnk`, the folder its program is installed in. Left out,
`~/.lnk/exec/bin`.

### `LNK_VERSION`

A specific version such as `0.14.0`. Left out, the latest. It is also the way
past a latest release older than the server's, which the installer refuses
while one is being published.

### `LNK_INSECURE=1`

Installs even without `ssh-keygen`, with no signature check: HTTPS and the
checksum only. Without it, a machine lacking OpenSSH is refused.

### `LNK_RELEASES_URL`

Where the releases are. Left out,
`https://github.com/so-local/link-releases/releases`.

## Upgrade guidance

Ordinary interactive commands schedule the existing signed release check
at the default cadence, without waiting for it. Cached notices name the
available version and `lnk upgrade`. Run `lnk upgrade --check` explicitly
to refresh guidance on a server or in a script. The check installs nothing.

### Notice policy

The core module's `upgrades.toml` owns the check interval, each notice
level's cadence and the levels quieted by `LNK_UPDATE_NOTICE=off`.
Ordinary and recommended notices are quiet with JSON output and without
a terminal. Critical advisories appear on stderr on every user command,
including JSON and nonterminal use, and never stop the command. Failed
checks retain previously verified advisory guidance.

### Release notes

A release carries its notes in the core module's `notes.toml`, beside
`upgrades.toml`: a `[[release]]` table for each release, newest first,
with its `version` and its `notes`, one line each. `lnk upgrade` records
the version it upgrades from in the update cache before it puts the new
release in place. The new `lnk`'s first command then prints, on stderr
and before its own output, the notes of every release after that one, up
to its own, oldest first, as a line naming each release and its notes as
a list. It records its own version as noted and stays quiet afterward.

#### When the notes show

The notes show where a newer-release notice would. With `--json`,
without a terminal, or with `LNK_UPDATE_NOTICE=off`, they wait for a
command that can show them, so none are lost, and a second upgrade
before then shows both releases' notes. A new install and a downgrade
show none. They never show inside a module's run, an `lnk` a module
runs with `LNK_EXE` set, and where the update cache's lock can't be
taken, a warning says so and the notes wait.

### Command history

The core's `history.toml` records the modules renamed and removed, and
the core's own retired spellings, in [the schema every module's `history.toml` keeps](../../../../../packages/spec/docs/more/libraries/types/specifications.md#history).
Each module answers its own retired commands and flags. Link 0.1 is the
baseline: the core's history starts empty there, and records only what a
later release retires or renames.

#### History entries

Each entry
records its retirement version, the release that deletes it and its
replacement or explanation. Commands and flags are refused with usage
status and the replacement to type. A retired variable set when a command runs
is a warning naming the variable without exposing its value, and the
command runs. History matching ignores flag values and
arguments following `--`. A specification's flags are retired by [its specification's schema
steps](../../../../../packages/spec/docs/more/libraries/types/specifications.md#retired-flags) instead.

#### Renamed modules

A `[[renamed]]` entry records a module's old name in `module`, its name
in `now`, the release it changed in `since` and the release that deletes
the entry in `until`, such as `module = "notes"`, `now = "notes-tool"`,
`since = "1.2.0"` and `until = "1.5.0"`.

#### Upgrading a renamed module

`lnk upgrade` reads the incoming core's history through its internal
`--link-history` query, as it reads its registry through
`--link-registry`. For each rename whose old program, `lnk-<module>`,
is in `lnk`'s folder and whose new name is in the incoming registry, it
installs the new module with the others, puts it in place, and then
removes the old program, so an interrupted upgrade leaves the old one
working. The installed modules are the programs in that folder, so
their list now names the new module. Renames chain: a module renamed
twice is replaced by its latest name.

#### Stopping the old module

Before its program goes, the old module is stopped as
[`lnk mod remove`](../../../../../docs/more/modules/guide.md#add-or-remove-a-module)
stops one (its `uninstall`), unless its new name's module declares no
removal, which keeps and runs nothing. One that can't stop, or can't
list what it keeps (`uninstall --parts`), keeps its program, else what
it runs would run on with nothing left to stop it, and the upgrade
names it with the `lnk mod remove` that removes it once it can.

#### What the old module ran

What stopping a renamed module stopped is said, with the new module's
`--help`, which says how to start it again. A refusal naming a bridge
release names `lnk upgrade <bridge>` as the next step, not the same
upgrade again.

#### Removed modules

A `[[removed]]` entry records a module a release no longer ships: its
name in `module`, why it's gone and what to use instead in `why`, and
its `since` and `until`. For each removal whose program is in `lnk`'s
folder and whose name isn't in the incoming registry, `lnk upgrade`
stops the module and removes its program once the release is in place,
and says why, keeping one that can't stop as a rename does. A module
renamed to one removed since is removed by its old name the same way.

#### A removed module's name

A removed module's name, or one renamed to it, is refused with usage
status and its `why`, as the command group it was and given to `lnk
module add` or `lnk mod remove`, which takes its program instead
while one an upgrade couldn't stop is left. A module is renamed or
removed, never both, and a removed module is no module or command of
the release.

#### The history query

`lnk upgrade` asks the downloaded core for the release's history, and a
module its own core, through `--link-history`.

##### The history query's shape

The `--link-history` answer is the release's history: a retired
spelling's `command`, `since`, `now` and `why`, a rename's `module`,
`now` and `since`, and a removal's `module`, `why` and `since`, with no
`until`, which only the shipping release's own checks need.

##### Each module's retired spellings

The answer also holds each module's retired spellings under `modules`,
by module. A module asks for them [to follow the names its specifications and
conversations hold](../../../../../packages/spec/docs/more/libraries/types/specifications.md#names-that-follow-history).

##### The history protocol

The answer is written in a version of the history protocol, declared in
`link-spec`'s `history/protocol.toml` and read into a
[`link_version::Protocol`](../../../../../packages/version/docs/more/libraries/types/version.md#protocol-versions)
(`link_spec::history::PROTOCOL`), and a change to its shape advances
the protocol.

##### The asker's version

`lnk upgrade` states its own version in
`LNK_HISTORY_PROTOCOL`, and the downloaded core refuses an older one
than it accepts, naming `lnk upgrade`, or first its bridge release once
its [`oldest`](../../../../../packages/version/docs/more/libraries/types/version.md#raising-the-oldest) is raised.
It answers in the version `lnk upgrade` stated, and states it in
`protocol`, which `lnk upgrade` reads first with `Protocol::read`,
refusing an older or a newer one and leaving the installed `lnk`
unchanged. The `--link-registry` answer is
in the [description protocol](../../../../framework/docs/more/modules/protocol.md#the-description-protocol).

#### A renamed module's old name

The old name is refused with usage status wherever a command takes it:
as the command group it was, and given to `lnk mod add` or
[`lnk mod remove`](../../../../../docs/more/modules/guide.md#add-or-remove-a-module). The answer names the module's current name and the
command given with it. `lnk mod remove` takes the old program
instead while one an upgrade couldn't stop is left. The history refuses a rename to itself,
twice, in a circle, from a current module or command, or to a name that
isn't a module of this release or one removed.

## Core Environment Inputs

These inputs affect core installation and issue reporting. They do not contain
credentials, and release signature checks remain independent of download URLs.

### `LNK_GITHUB_URL`

Where an outside module's releases are read in place of
`https://github.com`, for isolated tests. It takes the locations
`LNK_RELEASES_URL` takes, read by the same rules: an
`https://` address, a bucket or a folder here (`file:///<folder>`), and
plain `http://` to this machine in a debug build alone. A repository's
latest release is at `<owner>/<repo>/releases/latest/download` inside
it, as on GitHub. The module's pinned signer is still checked.

### `LNK_NO_BROWSER`

Set any value to print the issue form's fields without opening a browser.
This lets a terminal-only session retain the report and submit it separately.

### `LNK_TEST_RELEASE_SIGNERS`

A debug build reads this allowed-signers file in addition to the built-in
release signer. Release builds ignore it. Use it only with an isolated test
installation whose release artifacts the test owns.
