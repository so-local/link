# Modules Guide

**`lnk` is a small core, and each part of Link is a module**, so you install
only the parts you use. The core only
installs, runs, updates and removes them. You need `lnk`, which is free
and which [Install](install.md) covers, picking your first ones.

```sh
lnk mod list
lnk mod show telegram
lnk mod add telegram
lnk upgrade
```

## Inspect a Module

Use `lnk mod show telegram` to see whether the module is installed and
what it does. For an outside module, the same command shows its repository
and trusted signer's fingerprint. An unknown name is refused with a
pointer to `lnk mod list`; inspecting a module installs nothing.

## What Each Choice Installs

Picking a part installs only its modules and what they need, and a few
more come when you use them. The agents choice's tools are `code-tool`,
`file-tool`, `run-tool` and `scheduler-tool`, and it needs `sandbox`,
`mcp` and `manual`.

| You pick | Modules | `lnk` groups |
|---|---|---|
| [agents](../../../modules/agents/agent/docs/agents/README.md) | `agent`, `harness`, their tools and needs | `agent`, `sandbox`, `mcp`, `manual` |
| [boxes](../../../modules/compute/box/docs/services/boxes/README.md) | `box`, `cloud` | `box`, `cloud` |
| [documentation](../../../modules/tooling/manual/docs/tooling/manual/README.md) | `manual` | `manual` (short `man`) |
| [models](../../../modules/agents/agent/docs/agents/models/agent/README.md) | `model` | `model` |
| [storage](../../../modules/storage/storage/docs/services/storage/README.md) | `storage` | `storage` |

The rest come when you need them:

- A harness (`deepseek`, `hermes`, `openclaw`) when you [`lnk agent harness set`](../../../modules/agents/agent/docs/agents/harnesses/use/README.md#switch-in-one-command) it.
- A connection module (`telegram`, `slack`, `discord`, `signal`) when
  you connect one.
- `memory-tool` when you turn on your agent's memory (`lnk agent memory
  start`).
- `web-tool` with [`lnk mod add web-tool`](#add-or-remove-a-module), for
  [Link Web](../../../modules/agents/tools/web-tool/docs/agents/tools/web/README.md).
- `browser-tool` with [`lnk agent tool add browser`](../../../modules/agents/agent/docs/agents/README.md#tools), for
  [Link Browser](../../../modules/agents/tools/browser-tool/docs/agents/tools/browser/README.md).
- `desktop-tool` with `lnk agent tool add desktop`, for
  [Link Desktop](../../../modules/agents/tools/desktop-tool/docs/agents/tools/desktop/README.md).
- `aws` or `gcp` when you connect that cloud.
- `secret` when you first keep a secret with [`lnk secret create`](../../../modules/security/secret/docs/config/secrets/README.md#create-a-secret), for an
  agent to be [granted it](../../../modules/security/secret/docs/config/secrets/README.md).

## Add or Remove a Module

`lnk mod` lists, adds and removes modules, each with the modules it
needs, and `lnk` offers a module when a command needs one. `mod` is
short for `module`, the group's full name.

```sh
lnk mod list
lnk mod add telegram
lnk mod remove storage
```

For a released `lnk`, `add` installs from its own version's release and includes
required modules. For example, `agent` needs `sandbox` and `mcp`,
`bucket-store` needs `storage`, and `aws`, `gcp` and `box` need `cloud`.
[A source-built `lnk`](../developing/setup/install.md#add-modules-with-cargo) builds modules from its checkout instead.

### What Counts as Installed

`add` skips a module only when its program is already beside `lnk`, in
the folder `add` installs into. A program of the same name elsewhere on
your `PATH`, such as a build from source in a checkout's `target/debug`,
doesn't count, so `add` still installs its own from the release `lnk`
reads. `lnk` runs the program beside it first, so the one `add`
installed is the one run. [Module Installation](../../../core/framework/docs/more/modules/installation/README.md#where-a-program-comes-from)
says which release `lnk` reads.

### Remove a Module

`remove` also removes modules that require the named module, including
indirect dependents. For example, `remove sandbox` removes `agent` and
stops its agents first. Recommendations are optional: `remove agent`
keeps its harness adapters and connections, which recommend it, and
keeps `sandbox`, which it requires. Removing a module stops what it runs
and keeps its saved data.

### A Module a Command Needs

Run a command whose module you don't have, and `lnk` offers to install it.
Outside a terminal, typed or in your script, it installs it without
asking and says so: the command named what you wanted. The same goes for
a module it needs that isn't installed, such as `cloud` for `box` or
`sandbox` for `agent`.

### Modules That Go Together

Some modules go with others: a box with `sandbox`, Link Harness with
`memory-tool`, `web-tool` and `measure`. [`lnk up`](install.md#pick-your-modules)
installs a use's modules and what they need, never these. The first
command of a module whose companions you don't have asks once in a
terminal; a no is kept
in `~/.lnk/safe/modules/offers.toml` and not asked again, and `lnk mod
add` adds one later. Without a terminal it says once what adds them.
Link Harness's scheduler, run, file and code tools are in the agents
choice beside it, so they come with it.

### A Feature That Needs Another Module

A feature that needs another module installs it the same way: `lnk agent
backup` and `restore` the storage module, `lnk agent move <cloud>` the box
and cloud modules, a bucket in a cloud account (not a folder's) the
cloud module. In a terminal it asks first, naming everything it will
install; a command
naming the module itself, such as [`lnk agent connect slack`](../../../modules/agents/agent/docs/agents/connections/connect/README.md#choose-an-app), `lnk agent harness set hermes` or `lnk agent tool add web`, doesn't ask. A module's own call to
`lnk` underneath installs nothing: a missing module there is named, with
the command that installs it.

## Add a Module from Outside Link

A module Link doesn't come with comes from its own GitHub repository, signed
by its maker's key. A harness adapter is one example. The harnesses Link comes with are in
[Use another harness](../../../modules/agents/agent/docs/agents/harnesses/use/README.md#use-another-harness).

```sh
lnk mod add --url https://github.com/example/lnk-example
lnk mod add --url https://github.com/example/lnk-example --yes
lnk mod list
lnk mod remove example
```

- `add` installs the module from the repository's latest release. The
  first time, it shows the release's key and asks, and `lnk` then keeps
  the repository and the key in `~/Link/Safe/settings.toml`. With
  `--signer ssh-ed25519 ...`, it trusts the key you got from its maker
  instead of the one the release offers, and refuses a release signed
  by any other.
- [`lnk upgrade`](../../../core/cli/docs/more/architecture/files.md#lnksafereleasesupdatejson-600) updates it from the same repository, and only with a
  release signed by that key.
- Only a module can be added this way: its `lnk-<name>` must answer
  `--link-describe` as itself, a module and not a program of its own,
  and its name can't be one of Link's modules'. [A harness program](../../../modules/agents/harnesses/portability/contract/docs/agents/harnesses/more/protocol/README.md#harness-programs-lnk-name) is
  one.
- It runs as you, like any program you install. Link's sandbox confines
  the harness it installs, [not the module itself](security.md#modules-from-outside-link).
- [`lnk agent move`](../../../modules/agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud) to a box adds it there from the same repository,
  trusting only the key this computer trusted.

## Upgrade

`lnk upgrade` updates `lnk` and every installed module, only from a
release whose signature it checked.

```sh
lnk upgrade
lnk upgrade 0.0.1
lnk upgrade --check
```

Each release is checked against its signature first, and if anything
fails, what you have stays as it was. Its signed version is read before
anything else: a release of the version you have, every module
included, downloads nothing.

### Checking Without Installing

`lnk upgrade --check` installs nothing. It says whether a newer `lnk` is
out, then checks each module added with `lnk mod add --url` against
its own repository's latest release, signed by the key you trusted, and
names each newer one. Each part is checked whatever another found. When
one can't be made, such as a release host that doesn't answer, it says
why, and the check fails after naming what it did find.

### Your Yes

`lnk upgrade` first asks the release's own programs what they will
change in the files Link writes. Each tries its conversions without
writing, so a file that won't convert stops the upgrade before anything
is installed. It shows what it installs, each file it converts to the
new release's shape and each setting that moves to its new owner, and
asks once. A no installs nothing. History, such as conversations and
logs, is never rewritten, and a specification you wrote is only offered for
[`lnk spec convert`](../../../modules/tooling/spec/docs/config/specs/guides/kinds/README.md#convert-a-file).
With no terminal to ask on, the upgrade is refused unless `--yes` agrees
ahead.

```sh
lnk upgrade --yes
```

After your yes it installs the release, and each program then [makes
only the changes you agreed to](../../../packages/files/docs/more/libraries/files/files/upgrades.md).
A file holding a setting whose new owner isn't installed is kept as it
was and named, and the other changes go on. An upgrade stopped partway
finishes when `lnk upgrade` runs again, removing a renamed or removed
module's old program it left.

### Root's Copies

A Linux machine keeps root's own copies of two module programs in
`/usr/local/lib/lnk`, which root's services run: the keeper of your
secrets, `lnk-secret`, and a box's worker launcher, `lnk-sandbox`. A
new `lnk` can't talk to an old keeper, so `lnk upgrade` compares each
copy with your new one and, where they differ, offers to copy it again
with sudo, by [`lnk secret keeper create`](../../../modules/security/secret/docs/config/secrets/keeper/README.md#create-the-keeper)
and `lnk sandbox workers --install`. `--yes` copies them without asking.
Off a terminal, the upgrade names both commands as its next steps
instead.

```sh
lnk upgrade --yes
```

The list of copies and their commands is `root_programs.toml`, beside
the [upgrade's code](../../../core/cli/src/root_programs.rs).

### Update Notices

`lnk up` and [`lnk agent show`](../../../modules/agents/agent/docs/agents/models/agent/README.md#set-its-model-and-how-hard-it-thinks)
show ordinary notices of a newer release at most once a day, and
never with `--json`, off a terminal, or with `LNK_UPDATE_NOTICE=off`.
[Critical security advisories](../vulnerabilities.md#security-update-notices)
remain visible under those settings. Notices never wait for the network: a day after
the last check, they start `lnk upgrade --check` in the background, and
the next command says what it found.

### Release Notes

After `lnk upgrade`, the new `lnk`'s first command [prints what changed
in each release](../../../core/cli/docs/more/architecture/files.md#release-notes) since the one you had, once, and stays quiet afterward.
Like an ordinary notice, the notes wait for a command on a terminal,
without `--json` or `LNK_UPDATE_NOTICE=off`.

### On a Box Made from Your Own Build

A box made from a bundle of your own build keeps a copy of that one
release, and its `lnk` follows the copy. There `lnk upgrade --check`
says the copy holds no later release, and the box gives no notice of
one. `lnk upgrade` to any other version says the same and stops, naming
how the box gets another build: on your own machine, `lnk box create
<cloud> --bundle <bundle>` makes it again from a new bundle.

### Installed with Homebrew

Installed with Homebrew, `lnk upgrade` refuses, saying `lnk` was
installed with Homebrew and to use `brew` to change it: `brew upgrade lnk` upgrades it.

## Uninstall

`lnk uninstall` has each module stop what it runs, such as your agents
and their sandboxes, and removes `lnk` and its modules. Everything Link kept stays
unless you pick it, so installing again picks it up.

```sh
lnk uninstall
lnk uninstall --yes
lnk uninstall --yes --delete agents
lnk uninstall --yes --all
```

### What Each Part Takes

Installed modules report the parts they keep before you confirm removal.
Common examples are
`agents`, your agents, their memory, settings and keys, and what agents
that left kept (`~/.lnk/data/agents/move`); `personal`, your files in
`~/Link/Files`; `buckets`, bucket settings and encrypted buckets'
passwords; `clouds`, cloud keys and Link's SSH key and authority for your boxes;
and `rest`, everything else in `~/.lnk` and
`~/Link`. `--all` picks all of them.

### Before You Pick a Part

Move an agent you want to keep to another machine first with [`lnk agent move`](../../../modules/agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud), because `agents`
deletes its memory and conversations. `personal` deletes files you
never pushed to a cloud, and `buckets` deletes the only copy of
encrypted buckets' passwords. `clouds` leaves your boxes running, and
billing. `rest` takes folders you made in `~/Link` too. On a Mac, each
part's Keychain items go with it.

### Installed Another Way

Installed with Homebrew, it prints the command that removes the programs.
Installed with cargo, it prints `cargo uninstall` with the crates cargo
installed them from, and deletes any it doesn't track.

## Get Help

`lnk help` lists every command, and says what each one takes.

```sh
lnk help
lnk help storage
lnk help storage push
lnk mod list
```

When a command fails, [`lnk issue create`](../../../core/cli/docs/tooling/issue.md#report-a-bug)
turns the failure into a bug report with what's private scrubbed out.

## Where Things Live

Local Link keeps what's yours in the Link folder, `~/Link`, and what this
machine needs in `~/.lnk`. [`lnk location list`](../../../core/cli/docs/config/locations.md#list-the-locations)
lists each location on this machine, and
[`lnk location set`](../../../core/cli/docs/config/locations.md#set-a-location)
moves one, the Link folder included.
[Where Link Keeps Things](../reference/locations.md) explains both
folders, every location in them, and how to move one.

## More About It

What to do when a module won't install or update is in
[Troubleshooting](troubleshooting.md#modules). What's checked before anything installs is in [Security](security.md).
Why it works this way is in [Decisions](decisions.md). How `link-core`
finds, checks and installs each program is in
[Module Installation](../../../core/framework/docs/more/modules/installation/README.md).
How to build a module is in [Modules under Developing](../developing/modules.md).
