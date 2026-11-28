# What's on a Box

What a box holds, how it installs `lnk`, how its working users and its
people are kept apart, and how a specification and a setup reach it.
[Security](../security.md) says what each of them can reach.

## What's on a box

What says a machine is a box, and what it holds once it's made.

### A box's ID and name

They're in the box's `~/.lnk/safe/installation.toml`, written by `lnk box create`
over SSH before `lnk` first runs there, never in the first boot's
metadata, and in its tags.

### That it's a box

`/etc/lnk/kind.toml`, root's, written at its first boot (cloud-init, as
root) by `lnk box create`, says it's a box, and its OS and processor.
Written by anyone but root, it's refused, so no one but root can make a
machine a box; without it, a machine is a computer.

### Installed programs

Box installs `lnk`, Secret and Sandbox, and other modules come when
their capabilities are needed. The first boot (cloud-init) holds nothing
secret, since a cloud's metadata is readable from the machine, turns on
lingering for `ubuntu`, and installs nothing.

### Retrying apt

A new box's package mirror can be slow or end a download midway. So
the first boot writes `/etc/apt/apt.conf.d/80-lnk`, root's, before
anything else runs. apt (Ubuntu's package manager) reads that file on
every run. It then fetches a failed download again up to 3 times, and
drops a connection that sends nothing for 60 seconds, then retries it.
Every apt on the box behaves this way, Link's own installs and a
setup's `lnk box ssh <box> -- sudo apt-get install` alike.

```text
Acquire::Retries "3";
Acquire::http::Timeout "60";
Acquire::https::Timeout "60";
```

The values come from `link-machine`'s `apt.toml`. Link's own
installs on a computer, which has no such file, give apt the same
values on its command line.

## The install on a box

A box installs `lnk` from the releases this machine's `lnk` uses:
Link's own, unless `LNK_RELEASES_URL` names others, at `LNK_VERSION` if
set (a box specification's `[lnk] version` wins). Whichever they are, they're
trusted only as this machine's `lnk` trusts them, by the keys it was
built with.

### Checked here first

Before the cloud is asked for a box, `lnk box create` fetches the
release's `install.sh`, `VERSION`, `SHA256SUMS` and `SHA256SUMS.sig`
here, and goes on only when the sums are signed by a key this `lnk`
trusts (a release's trusts Link's signer alone), the script's
checksum is the signed one, and the release is one [`lnk upgrade`](../../../../../../../docs/more/modules/guide.md#upgrade) would
install.

### Which release

The version asked for (a specification file's or `LNK_VERSION`'s, neither behind
a released `lnk` doing the asking, or one typed with
`--lnk-version`, which may be behind on purpose), or the latest, which
must say its version and be at or above that `lnk`.

### Its signed version

A release whose signed `VERSION` is missing, or says another version
than the one asked for, is refused here, and what it installs is
checked again on the box.

### The script on the box

The box runs that script as it was checked, copied into its setup: it
fetches no script of its own, from a relay or anywhere. It runs it at
the version checked, the latest's too, so the box installs that version
or nothing, whatever the latest is by then.

### Checked again there

The script checks the release's signed `VERSION`, and
the version the `lnk` inside reports, against it; then the setup checks
again that the `lnk` installed says it's that version, and removes it
and fails if not. So an old signed release put under a newer version's
tag installs nothing.

### What it adds

That `lnk` checks its archives and modules with the keys the script
holds, which were signed with it, and adds the accounts, secret and
sandbox modules, the Linux keeper and the worker launcher.

### `LNK_RELEASES_URL`

When it's set, every `lnk` that `lnk box run` runs there follows it, so
a move's upgrade installs what this machine did, checked by the box's
own `lnk`. A debug build also takes `http://` to this machine. A box
given a copy of releases it can't read, such as a bundle or a folder
here, follows that copy for good instead, whatever this machine follows
later, and so does every `lnk` typed on the box itself.

#### What it may be

It's `https://` with no login (`user@`), `\`, query or fragment, a
bucket of yours (`s3://`, `gs://`) or a folder here (`file://`);
anything else fails. The box can't read the last two, so `lnk box
create` copies it the checked release over SSH, as
[Where Releases Are Read From](../../../../../../../scripts/docs/more/developing/releases.md#where-releases-are-read-from) explains.

### Before an agent comes

Before [`lnk agent move`](../../../../../../agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud) or [`lnk agent create --on`](../../../../../../agents/agent/docs/agents/start/create.md#create-on-a-hosting-account), the harness module
adds itself, the sandbox, the secret module and the model module, which
serves every model call, there (with the storage module for an agent's
backups), and [`lnk sandbox check --ready --yes`](../../../../../../security/sandbox/docs/tooling/sandbox/guide.md#sandbox-guide) installs bubblewrap
with apt if it's missing and adds the AppArmor profile letting it make
user namespaces, as [`lnk agent start`](../../../../../../agents/agent/docs/agents/README.md#start-your-agent) offers on Ubuntu, through the
box's `sudo`, which asks nothing.

## Working users

An ordinary box keeps trusted controllers and proxies under its owner,
while each sandboxed agent runs as a fresh locked Linux user. Agents are
sandboxed by default. An owner may explicitly use `lnk agent start
--no-sandbox`; then the box remains the outer isolation boundary and the
harness runs with the rights of that agent's box user. A trusted
`unsandboxed = false` in the box user's `agents.toml` refuses that choice.

### A fresh user

Its groups are fresh too: the controller's `sudo` membership is never
inherited. The first program inside is root's installed sandbox helper;
it drops supplementary, real, effective and saved IDs, clears all
capabilities and sets `no_new_privs` before confinement and workload
code. A worker cannot use sudo or read the keeper's or controller's
private files outside its grants.

### Its view of the state

The launcher makes a private mount namespace and clones the granted
state with kernel identity mappings. Controller-owned inodes appear as
the worker's there; new `0600` files remain controller-owned
underneath. Replacement, backup and moves can therefore read private
state without recursive ownership changes or lasting ACL grants.

### What it can't see

Read-only grants stay read-only, and nested mounts keep covered data
hidden. The worker's empty home is a private tmpfs. The installed
helper is bound read-only; its executable does not need a mapped
filesystem.

### What the kernel needs

The kernel must support `openat2`, pidfds and recursive idmapped mounts
(Linux 5.12 or newer, with filesystem support); the worker requires
seccomp on x86_64 or arm64. Unsupported grants fail before any workload
runs. The launcher uses the kernel mount API, without depending on a
newer version of the distribution's mount command.

### Its numbers

Root journals setup before account mutations in `/var/lib/lnk/workers`.
Its durable `next-id` counter reserves UID/GIDs above automatic account
and subordinate-ID ranges, skips existing delegated ranges, and never
reuses a retired identity. Subordinate allocation must use files, not
an external NSS provider. Missing, malformed or exhausted state refuses
new work.

#### Keeping the range

On an existing or custom box that range must never have been delegated
to an untrusted user namespace. Preserve the counter and range across
upgrades; never reset it or delegate retired IDs while older file
capabilities could survive.

### Old descriptors

A replacement cannot gain fresh path access through an old worker's
retained idmapped directory descriptor. Already opened file
capabilities (including directory listings) follow Unix semantics; a
run may copy or share data and plain secrets explicitly granted to it.

### Cleanup

The supervisor pins both its launch front and controller with pidfds
across sudo. Normal completion, signals and either process's death kill
and reap every worker process before account removal. Cleanup uses
pinned process handles, validates journalled UID/GIDs and detaches
mounts before removing only their empty mount points.

### Cleanup it can't prove

If process cleanup cannot be proved, the account stays. Later failures
retain the remaining journal or runtime cleanup for the next launch and
boot's `lnk-workers-reap` service to retry. Sandboxed programs cannot
create setuid or setgid artifacts in state; `openat2` receives `ENOSYS`
so callers use the inspected `openat`.

### Its setup

Setup installs `/etc/lnk/workers.json` and root's
`/usr/local/lib/lnk/lnk-sandbox`. After upgrading the sandbox module,
refresh it through [Worker Controller Maintenance](../../../../../../security/sandbox/docs/tooling/sandbox/more/workers.md)
as the box controller.
A sudo command default preserves the launcher's explicit descriptors
and PATH; it grants workers no sudo authorization.

### Other users

Named `lnk box user` users retain their existing separate, non-sudo
identities, validated against root's user record. When their writable
folder contains their normal settings, session control files live in a
private temporary directory outside it and are removed on normal exit.
Any other user of an ordinary box, root included, is refused a sandbox
("only the box controller launches work"). Packed machines retain their slot-user boundary and declare that mode
in root's worker settings.

## A box specification

A specification may be someone else's, so it's checked when it's read, before
anything is asked of a cloud. A specification has no paths, no scripts and no
account: nothing runs from it.

### Every value is typed

`[lnk] version` is a version (`0.0.1`, or a prerelease as
`0.0.1-staging.202610131900`); a region and a machine type
are lowercase letters, digits, dashes and dots; the OS, a GPU model and
a disk type are words from Link's lists; `name` has a box name's
characters.

### `[lnk] version`

From a file, the specification's [`[lnk]`](../../../config/specs/box.md#lnk)
table can't be behind this computer's `lnk`. It reaches the box's setup
script quoted, as `install.sh`'s `LNK_VERSION`.

### Your tags

They keep to [the tag rules](../../../config/specs/guides/box.md#values): at most
40, so Link's own always fit (AWS takes 50), and none named as Link's
or the cloud's. On Google Cloud they're labels, never network tags,
which the firewall's rules match.

### To the cloud's CLI

Values reach the cloud's CLI as separate arguments, and its JSON
(`--block-device-mappings`, `--tag-specifications`) is built by a
serializer.

### Asked first

`lnk box create` says the machine, its disk, the `lnk` it gets and the
price when the cloud gives one, then asks; `--yes` takes it unasked. A
pinned size has no `max` to keep it small, and a fast disk is billed
for its speed too.

### What `lnk box spec show` prints

`lnk box spec show <box>` is read from this computer's `boxes.toml` and what
the cloud says of the box, never from the box, where whatever runs
could write the specification your next boxes are made from. It's printed by the
`toml` serializer, so a name from a cloud can't add a line.

## Setting a box up

What your setup script and its stages can reach. [`lnk box copy`](../ssh.md#copy-files-to-a-box)
puts a setup's files on the box, and its page says how it writes them.

### A setup script

It runs on this computer, as you, with everything you can do: Link runs
none of it. A script someone else wrote: read it first.

### Stages

Stages are what your script makes them. Each runs in its own sandbox
specification, started by [`lnk sandbox run --spec`](../../../../../../security/sandbox/docs/tooling/sandbox/guide.md#sandbox-guide)
on the box, and reaches only what its specification opens,
so a stage installing dependencies can't reach your repository's host,
and one building reaches no network, as the
[sandbox's security](../../../../../../security/sandbox/docs/tooling/sandbox/security.md) guarantees.
A stage outside any sandbox, as installing system packages with `sudo`
must be, runs as root there.

#### Folders a stage writes

Write `lnk box copy`'s folder without a `~`, which your shell would turn
into this computer's home folder. A stage's sandbox makes a folder its
specification writes that isn't there yet, so a `mkdir -p app` stage only makes
it sooner. A stage can't write a dot folder in your home, such as
`~/.cache`, so keep a stage's caches in its own folder or turn them off,
as `--no-cache-dir` does.

#### Dot files a stage keeps

A stage keeps the dot files that run outside it read only at the top of
each folder it writes: `.git`, `.envrc`, `.vscode` and `.idea`. On Linux
it holds an empty one in place of each that isn't there, so `git clone`
into the folder finds it not empty. The clone stage names the folder in
`write_dot_files`, so cloning writes the repository's `.git`, and the
stages after it keep that `.git` read only.

## A box's users

A box's user (`lnk box user add`) is a Linux user of their own, made by
root with their own home, the one folder their sandbox writes, in the
group `lnk-users`. Their login shell is `/bin/sh`, which reads no file
of theirs before the command sshd runs.

### Root's record of them

Root keeps each in `/etc/lnk/users/<name>/` (755, its files 644,
root's): their folder and key, their sandbox's specification with its paths
absolute, and `authorized_keys`, whose one line is
`restrict,pty,command="/usr/local/lib/lnk/lnk-sandbox enter
--spec <spec> --folder <home>" <key> lnk-user-<name>`.

### Their keys, root's file

sshd reads that file for the group
(`/etc/ssh/sshd_config.d/60-lnk-users.conf`), never their home's
`.ssh`, which their sandbox writes: nothing they run lets in another
key or one without the sandbox. The drop-in turns off forwarding,
`~/.ssh/rc` and `~/.ssh/environment` for them too, as `restrict` does
on the line. A file there that anyone but root could write is refused.

### Their sessions

A shell, a command, `scp` or `sftp` all run in their sandbox, as
[Sessions over SSH](../../../../../../security/sandbox/docs/tooling/sandbox/security.md#sessions-over-ssh) explains.
The programs their sessions run are root's copies of `lnk` and
`lnk-sandbox` in `/usr/local/lib/lnk`, since the box's own user's home
is closed to the rest; each `add` brings them up to the `lnk` that ran
it.

### Adding and removing them

Their sandbox is shown and asked about before they're added (`--yes`
skips it), read as their session reads it. A name that's already a
Linux user `lnk box user add` didn't make is refused, as is root: it
never takes over a user. `lnk box user remove` takes their key away
first, then their user; their home stays unless `--delete-folder`,
made root's alone (owner and group root, mode 700), so a user given
their user ID later owns none of it.
