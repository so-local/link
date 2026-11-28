# Where Link Keeps Things

Local Link keeps what's yours in `~/Link` and what it needs in `~/.lnk`.
This page explains why there are two folders, how each is split so no
agent can change what steers it, and where every location is.

```sh
lnk location list
```

## Two Folders

Local Link keeps nearly everything in two folders in your home folder.
Operating-system registrations, system installations and the `PATH` line
in your shell's startup file add a few more, listed under
[Outside Your Home Folder](#outside-your-home-folder).

### `~/Link`: What's Yours

`~/Link` is what you see and work in. It holds your files, your
agents' files and your teams' shared folders. Its `Safe/` holds the
files you edit that steer Local Link, such as each agent's settings and
grants. Anyone can find, read and manage this folder, programmer or
not. Three of its folders belong to no single owner:

- **`~/Link/Files`** is for your own files, which [storage backs up](../../../modules/storage/storage/docs/services/storage/README.md).
- **`~/Link/Logs`** keeps the record of each command you run.
- **`~/Link/Archive`** keeps what's gone, in a folder for each owner.

#### Inside `~/Link`

Each agent's specification and grants live in `Safe/Agents/<name>`, its
work in `Agents/<name>/home`, and its collections in `agent`. A box on
this machine with no VM works in its own `Boxes/<name>/home`. Local
Link's own files sit at the root of `Safe`, `Logs` and `Archive`. Local
Link keeps no hidden file or folder in `~/Link`, so even a binary index
stays visible beside the data it describes.

```text
Link/
├── Safe/
│   ├── folder.toml
│   ├── settings.toml
│   ├── Agents/main/
│   ├── Agents/defaults.toml
│   ├── Teams/
│   ├── Models/models.toml
│   └── Sandboxes/presets/
├── Agents/main/
│   ├── home/
│   ├── agent/
│   └── Memory/
├── Teams/research/
├── Boxes/here-1/
│   └── home/
├── Files/
├── Logs/
└── Archive/
```

### `~/.lnk`: What Link Keeps

`~/.lnk` is what Local Link keeps for itself. Its `safe/` holds what
Local Link writes that steers it, such as credentials, approvals and
journals. Its `exec/` holds the programs Local Link installs, and its
`data/` holds the rest, such as the homes of your agents' harnesses.
Developers and administrators can inspect it. It holds important records
and credentials, so it is never a disposable cache.

#### Inside `~/.lnk`

Each owner has its own folder in `safe`, `exec` and `data`, and the files
directly in `safe` are the core's. `exec/bin` holds only `lnk` and its
modules, while harness installations and shims sit under their owners
in `exec`. Harness homes, portable continuation and memory live under
`data`, where a caller may grant them. Installed programs are writable
only outside a run, and a run reads only the programs its grants name.

```text
.lnk/
├── safe/
│   ├── map.toml
│   └── <owner>/
├── exec/
│   ├── bin/
│   └── <owner>/
└── data/<owner>/
```

### State and Prerequisites

The split follows one question: what would someone else need to have
the same thing? A thing's state is its configuration and files, which
you'd lose with the machine. What it needs to run, such as an installed
harness, a credential or an approval, is a prerequisite, which each
machine sets up for itself. State is kept under `~/Link`, and
prerequisites in `~/.lnk`. So an
[archive](../../../modules/tooling/archive/docs/tooling/archive/README.md)
carries a thing by copying its folders under `~/Link` and nothing else.

## Safe, Exec and Data

Each folder in either place is one of three kinds, so that an agent
can never widen its own access:

- **Control** steers Local Link: a setting, specification, preset,
  grant, credential, approval or saved yes. Control lives only in a
  Safe, `~/Link/Safe` or `~/.lnk/safe`.
- **Exec** is what a run may execute: `lnk`, its modules and the
  harnesses Local Link installs, in `~/.lnk/exec`. `~/.lnk/exec/bin`
  holds only what goes on your `PATH`, `lnk` and its modules.
- **Data** is everything else, what a run may be granted. Local Link
  never reads control from it.

A sandbox is the walls Local Link puts around a program it runs. Local
Link makes both Safes private before a run starts. Every sandbox hides
both Safes whole, for reading and writing, and lets a run only read
`exec`. So no agent can change what steers it, even a setting
that isn't secret, and no agent can plant a program another runs.

## One Folder per Owner

Each kind of thing, such as agents or boxes, has an owner, the module
that keeps it. An owner has one folder in each part, named by its plural
word, and each thing has its own folder inside. An agent `main` has
four, at the same path in each part:

```text
~/Link/Agents/main/
~/Link/Safe/Agents/main/
~/.lnk/safe/agents/main/
~/.lnk/data/agents/main/
```

Each part keeps only the files that fit its kind, so the four folders
hold different files. [Agent Files](../../../modules/agents/agent/docs/agents/state/files.md)
lists what an agent keeps in each, and the
[Secrets guide](../../../modules/security/secret/docs/config/secrets/README.md#where-a-secret-is-kept)
says where a secret's value goes.

### Folder Names

A folder directly in `~/Link`, or in `~/Link/Safe`, which mirrors it, is
Title Case (`~/Link/Agents`, `~/Link/Safe/Agents`, `~/Link/Safe/Models`).
Every folder below those is lowercase, as every folder in `~/.lnk` is:
`~/Link/Agents/main/home`, `~/Link/Safe/Sandboxes/presets`. Commands
stay singular, as in [`lnk agent`](../../../modules/agents/agent/docs/agents/README.md#agents).

### Names Kept for Local Link

Local Link's own files sit at each part's root, such as
`~/Link/Safe/settings.toml` and `~/.lnk/safe/installation.toml`. A name
an owner keeps in its folder for itself can't name one of its things:
an agent can't be named `journal`, since `~/.lnk/safe/agents/journal` is
the agent module's.

### Kinds and State Parts

Each owner in `layout.toml`'s `[owners]` names its `kind`, the word for
one of its things, and its `state`, the parts whose folder of a thing
is state:

```toml
# layout.toml
agents = { module = "agent", kind = "agent", parts = ["link", "link-safe", "safe", "data", "exec", "archive"], state = ["link", "link-safe"] }
```

The layout refuses state in `~/.lnk`, and a kind two owners share. A
test holds the rest: every owner's folder under `~/Link` is its state,
and no program or credential is kept there. It lists by name the
exceptions still left, a team's settings file in `Safe/Teams` and a box's
specification in `~/.lnk/safe/boxes`, until those kinds keep one folder per thing.

## Moving a Location

Every location has a stable ID, such as `link.files` or
`lnk.agent-data`. The map, `~/.lnk/safe/map.toml`, records where each
location is on this machine, as a path relative to its root, or an
absolute path for data kept elsewhere. Local Link writes every ID the first time
it starts, so a later release that changes a default leaves your machine
as it is. The map itself always stays at the same path.

### Set a Location

[`lnk location set`](../../../core/cli/docs/config/locations.md#set-a-location)
moves a location and everything inside it, then updates the map, and
points Local Link at files you moved yourself.

### Paths in a Moved Link Folder

Every path Local Link stores in the Link folder is relative to it, so
the folder can move anywhere in your home folder. When it moves, Local
Link points what your agents' harnesses keep at the new place. Each path
into the old one in their settings and state is rewritten, as it is when
an agent moves to another machine.

### What the Map Refuses

Reading the map refuses an entry that would weaken the folders' kinds:

- Control outside a Safe, where a run could reach it.
- A program outside `~/.lnk/exec`.
- Data inside a Safe or `exec`.
- Two locations that overlap where their defaults don't.
- An ID this release doesn't know.

The map names its schema, `schema = 1`, and a map written by a newer
release is refused with a request to run [`lnk upgrade`](../modules/guide.md#upgrade).

## Outside Your Home Folder

System services and system installations use these locations only when
those features are installed. The
[service contract](../../../packages/service/docs/more/libraries/runtime/services.md)
describes registration and shutdown. Many of these folders are shared
with the rest of the system, so look only at the entries Local Link owns.

### Checking a Clean Removal

Local Link removes what it installs, third-party harnesses included,
and keeps your saved data only when you choose. This section and
[Every Location](#every-location) let you check that by hand: stop the
programs, remove their registrations, then look at each location.
The map records moved folders, so check where the map says, not only
the defaults.

### `/etc/lnk/`

System settings include machine-kind information, service settings
and worker settings. This is protected control information, separate
from the current user's Safe.

### `/usr/local/lib/lnk/`

System-installed executables, including the secret keeper and sandbox
worker's programs, live here. The system layout in
[`layout.toml`](../../../packages/files/layout.toml) names the programs and their locations.

### macOS

macOS uses launchd for background services and the login Keychain for
secrets. User and system registrations are separate, and system state
has a separate root from the two folders in your home.

#### `~/Library/LaunchAgents/`

Per-user service registrations are plist files named by each service's
launchd label. Their program paths identify the executables they start;
removing a file alone does not stop an already loaded service.

#### `/Library/LaunchDaemons/`

System service registrations use this directory. Their lifecycle is
owned by the system service definition, and removing them requires the
corresponding system permissions.

#### `/var/db/lnk/`

This is Link's macOS system-state root. It can contain protected worker
records and system-service data, as declared by the system layout.

#### Login Keychain

Link-managed secrets use Keychain items under the service name `link`;
the [Keychain package](../../../packages/keychain/docs/more/libraries/files/keychain.md)
owns their format. Inspect the relevant items, rather than deleting the
Keychain file. A harness adapter's declared traces identify any additional
login material belonging to that harness.

### Linux

Linux separates user services from system services and keeps system
state under `/var/lib`. The configured service backend determines which
registrations exist; prepared guests can use a process supervisor instead.

#### `~/.config/systemd/user/`

User service units live here by default, or under
`$XDG_CONFIG_HOME/systemd/user/` when that variable is set. The service
records identify the unit names; stop and unregister those units before
checking that their files are gone.

#### `/etc/systemd/system/`

System service units are installed here by their declarations. This is a
shared system directory: inspect only Link-owned units and the programs
those units launch.

#### `/var/lib/lnk/`

Link's Linux system state includes the secret keeper's store in `secret/`
and declared worker records. Its data part, `/var/lib/lnk/data`, holds
state a run may be granted, in a folder for each owner by its plural
word, as in `~/.lnk/data`. Also check `/var/lib/private/lnk/`, the
private state alias used by systemd-managed services.

### Runtime Sockets

With `XDG_RUNTIME_DIR` set, sockets live under its `lnk/` directory.
Otherwise Linux uses `/run/user/<uid>/lnk/` when available; the fallback
on either operating system is `/tmp/lnk-run-<uid>/`. These are runtime
locations. Stop the processes using them before verifying their removal.

### Shell and Package-Manager Records

If you add the installer's suggested `PATH` entry, it goes in `~/.bashrc`
or `~/.zshrc`, depending on your shell. Cargo installations also have
executables in the chosen Cargo installation's `bin/` and package-manager
records; Homebrew tracks its own installation. [Installation Methods](../developing/setup/install.md)
explains these differences. Remove only the Link-specific entries and
use the package manager's records to identify what it installed.

## Every Location

Each entry is a location in [`layout.toml`](../../../packages/files/layout.toml): its
default path, its kind, its owner and its ID in the map. Paths are the
defaults a new machine starts with, and [`lnk location list`](../../../core/cli/docs/config/locations.md#list-the-locations) shows this
machine's.

<!-- inventory:layout:start -->

- `~/.lnk/data/agents/context`: data, agents', location `lnk.context`
- `~/.lnk/data/agents/harness-history`: data, agents', location `lnk.harness-history`
- `~/.lnk/data/agents/memory`: data, agents', location `lnk.shared-memory`
- `~/.lnk/data/agents/move`: data, agents', location `lnk.agent-move`
- `~/.lnk/data/agents/shared-harnesses`: data, agents', location `lnk.shared-harnesses`
- `~/.lnk/data/agents`: data, agents', location `lnk.agent-data`
- `~/.lnk/data/archives`: data, Link's own, location `lnk.archive-staging`
- `~/.lnk/data/bundles`: data, bundles', location `lnk.bundles`
- `~/.lnk/data/harnesses/claude`: data, harnesses', location `lnk.claude`
- `~/.lnk/data/harnesses/codex`: data, harnesses', location `lnk.codex`
- `~/.lnk/data/harnesses/configured`: data, harnesses', location `lnk.harness-configured`
- `~/.lnk/data/harnesses/openclaw/doctor-release`: data, harnesses', location `lnk.openclaw-doctor-release`
- `~/.lnk/data/measures/measures.jsonl`: data, measures', location `lnk.harness-measure-log`
- `~/.lnk/data/models/context`: data, models', location `lnk.model-context`
- `~/.lnk/data/teams`: data, teams', location `lnk.team-memory`
- `~/.lnk/exec/agents/shims/mktemp`: exec, agents', location `lnk.mktemp`
- `~/.lnk/exec/agents/shims`: exec, agents', location `lnk.shims`
- `~/.lnk/exec/bin/lnk`: exec, Link's own, location `lnk.core-program`
- `~/.lnk/exec/bin`: exec, Link's own, location `lnk.bin`
- `~/.lnk/exec/harnesses/claude`: exec, harnesses', location `lnk.claude-programs`
- `~/.lnk/exec/harnesses/codex`: exec, harnesses', location `lnk.codex-programs`
- `~/.lnk/exec/harnesses/deepseek`: exec, harnesses', location `lnk.deepseek-programs`
- `~/.lnk/exec/harnesses/hermes`: exec, harnesses', location `lnk.hermes-programs`
- `~/.lnk/exec/harnesses/openclaw`: exec, harnesses', location `lnk.openclaw-programs`
- `~/.lnk/safe/agents/bridges`: control, agents', location `lnk.shared-bridges`
- `~/.lnk/safe/agents/credentials`: control, agents', location `lnk.agent-credentials`
- `~/.lnk/safe/agents/environments`: control, agents', location `lnk.agent-environments`
- `~/.lnk/safe/agents/group-ids.toml`: control, agents', location `lnk.group-ids`
- `~/.lnk/safe/agents/harness-grants`: control, agents', location `lnk.harness-grants`
- `~/.lnk/safe/agents/ids.toml`: control, agents', location `lnk.agent-ids`
- `~/.lnk/safe/agents/installations`: control, agents', location `lnk.installations`
- `~/.lnk/safe/agents/journal`: control, agents', location `lnk.migration-journal`
- `~/.lnk/safe/agents/machines`: control, agents', location `lnk.agent-machines`
- `~/.lnk/safe/agents/places`: control, agents', location `lnk.agent-places`
- `~/.lnk/safe/agents/policy.toml`: control, agents', location `lnk.agent-policy`
- `~/.lnk/safe/agents/services.toml`: control, agents', location `lnk.agent-services`
- `~/.lnk/safe/agents/stopped-services`: control, agents', location `lnk.stopped-services`
- `~/.lnk/safe/agents/store-scratch`: control, agents', location `lnk.store-scratch`
- `~/.lnk/safe/agents/transfers/held`: control, agents', location `lnk.held`
- `~/.lnk/safe/agents/transfers/moved`: control, agents', location `lnk.moved`
- `~/.lnk/safe/agents/transfers/moving`: control, agents', location `lnk.moving`
- `~/.lnk/safe/agents/transfers/receiving`: control, agents', location `lnk.receiving`
- `~/.lnk/safe/agents/transfers/restore`: control, agents', location `lnk.restoring`
- `~/.lnk/safe/agents/transfers/scratch`: control, agents', location `lnk.transfer-scratch`
- `~/.lnk/safe/agents/transfers/sending`: control, agents', location `lnk.sending`
- `~/.lnk/safe/agents/transfers/teams`: control, agents', location `lnk.team-moving`
- `~/.lnk/safe/agents`: control, agents', location `lnk.agents`
- `~/.lnk/safe/approvals`: control, Link's own, location `lnk.approvals`
- `~/.lnk/safe/artifacts`: control, Link's own, location `lnk.artifacts`
- `~/.lnk/safe/aws`: control, aws', location `lnk.aws`
- `~/.lnk/safe/boxes/machines`: control, boxes', location `lnk.box-machines`
- `~/.lnk/safe/boxes`: control, boxes', location `lnk.boxes`
- `~/.lnk/safe/bundles/signers`: control, bundles', location `lnk.bundle-signers`
- `~/.lnk/safe/clouds/prices`: control, clouds', location `lnk.prices`
- `~/.lnk/safe/clouds`: control, clouds', location `lnk.clouds`
- `~/.lnk/safe/envs/jobs`: control, envs', location `lnk.jobs`
- `~/.lnk/safe/envs`: control, envs', location `lnk.envs`
- `~/.lnk/safe/gcp`: control, gcp', location `lnk.gcp`
- `~/.lnk/safe/installation.toml`: control, Link's own, location `lnk.installation`
- `~/.lnk/safe/maintenance.toml`: control, Link's own, location `lnk.maintenance`
- `~/.lnk/safe/map.toml`: control, Link's own, never mapped
- `~/.lnk/safe/measures/credentials`: control, measures', location `lnk.measure-credentials`
- `~/.lnk/safe/measures/measures.jsonl`: control, measures', location `lnk.measure-log`
- `~/.lnk/safe/measures`: control, measures', location `lnk.measures`
- `~/.lnk/safe/models/facts.toml`: control, models', location `lnk.model-facts`
- `~/.lnk/safe/modules/offers.toml`: control, Link's own, location `lnk.module-offers`
- `~/.lnk/safe/modules`: control, Link's own, location `lnk.modules`
- `~/.lnk/safe/releases/update.json`: control, Link's own, location `lnk.update`
- `~/.lnk/safe/releases`: control, Link's own, location `lnk.releases`
- `~/.lnk/safe/sandboxes/grants`: control, sandboxes', location `lnk.sandbox-grants`
- `~/.lnk/safe/sandboxes`: control, sandboxes', location `lnk.sandboxes`
- `~/.lnk/safe/secrets/secrets.toml`: control, secrets', location `lnk.secrets`
- `~/.lnk/safe/secrets/settings.toml`: control, secrets', location `lnk.secret-settings`
- `~/.lnk/safe/services`: control, Link's own, location `lnk.services`
- `~/.lnk/safe/settings.toml`: control, Link's own, location `lnk.settings`
- `~/.lnk/safe/storage`: control, storage', location `lnk.storage`
- `~/.lnk/safe/teams`: control, teams', location `lnk.teams`
- `~/.lnk/safe/upgrades`: control, Link's own, location `lnk.upgrades`
- `~/Link/Agents`: data, agents', location `link.agents`
- `~/Link/Archive`: data, Link's own, location `link.archive`
- `~/Link/Boxes`: data, boxes', location `link.boxes`
- `~/Link/Files`: data, Link's own, location `link.files`
- `~/Link/Logs`: data, Link's own, location `link.logs`
- `~/Link/Safe/Agents/defaults.toml`: control, agents', location `link.agent-defaults`
- `~/Link/Safe/Agents`: control, agents', location `link.agent-settings`
- `~/Link/Safe/Models/models.toml`: control, models', location `link.models`
- `~/Link/Safe/Sandboxes/presets`: control, sandboxes', location `link.sandbox-presets`
- `~/Link/Safe/Teams`: control, teams', location `link.team-specs`
- `~/Link/Safe/folder.toml`: control, Link's own, location `link.marker`
- `~/Link/Safe/settings.toml`: control, Link's own, location `link.settings`
- `~/Link/Safe`: control, Link's own, location `link.safe`
- `~/Link/Teams`: data, teams', location `link.teams`
- `~/Link`: data, Link's own, location `link`

<!-- inventory:layout:end -->

### A Module's Own Folders

A module released apart from Link keeps its folders in `~/.lnk` too,
in its own folder by its word, and lists them in its own documentation
rather than here. They aren't on the map, so each stays at its declared
path in this machine's `~/.lnk`, and `lnk location set` doesn't move them.
