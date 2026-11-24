# Files

The files `lnk cloud` and `lnk box` keep, and what each one holds. A
box specification's every field and default: [box specification](../../../config/specs/box.md).
`lnk help <command>` lists the environment variables each command reads,
as [`module.toml`](../../../../module.toml) declares them. Inspect the boxes tracked by your connected accounts.

```sh
lnk box list
```

## On this computer

What [`lnk cloud`](../../../../../../cloud/cloud/docs/services/cloud/README.md#cloud-accounts) and `lnk box` keep in Link's settings folder.

### `~/.lnk/safe/clouds/accounts.toml`

Mode 600: the connected accounts, and what each cloud's CLI needs to act
as them.

### `~/.lnk/safe/aws/` and `~/.lnk/safe/gcp/`

Mode 700: the AWS CLI and gcloud, when [`lnk cloud connect`](../../../../../../cloud/cloud/docs/services/cloud/guide.md#connect-a-cloud-account) installed
them, for you only and with no administrator rights.

### `~/.lnk/safe/gcp/credentials/<name>.json`

Mode 600: a Google Cloud account's credential file, a service account's
key or your login's application default credentials. Result-upload
signing passes the connected service-account file's path explicitly to
gcloud; no private key value is passed as an argument.

### `~/.lnk/safe/boxes/`

Mode 700: the boxes, Link's SSH key for them (`id_ed25519`), this
computer's SSH certificate authority (`ssh-ca.pub`, its secret in
`ssh-ca` off a Mac) and the certificates it signed (`certs/`), their host
keys (`known_hosts`), and lock files.

### `~/.lnk/safe/boxes/boxes.toml`

Mode 600: this computer's boxes, each with the specification it was made from
(`lnk box spec show`). A box here also has a `here` table: its `home` and its
`confinement`, either `{ kind = "sandbox", spec = "..." }` with the
sandbox specification captured at creation, or `{ kind = "direct" }`.

### `~/.lnk/safe/boxes/here/<name>.toml`

Mode 600: a box here's captured sandbox specification, written from its record
for each command [`lnk sandbox run`](../../../../../../security/sandbox/docs/security/sandbox/guide.md#sandbox-guide) confines, and removed with the box.

### `~/Link/Boxes/<name>/home`

Mode 700: a box here's home, where its commands start and what its
sandbox writes. It's your data: `lnk box delete` removes it only once
you confirm.

### `~/.lnk/safe/boxes/mux/`

Mode 700: the sockets of the SSH connections to boxes kept open for a
minute after a `lnk box run`.

## On a box

What `lnk box create` writes on a box, and what its users and workers
need there.

### `~/.lnk/safe/installation.toml`

Its installation ID and name, written by `lnk box create` (`installation_script`),
keeping an ID the box already has.

### `/etc/lnk/kind.toml`

Mode 644, root's: [that it's a box](on-a-box.md#that-its-a-box), and its OS and processor (`kind =
"box"`, `os`, `arch`), written as root at its first boot by `lnk box
create`. Without it, a machine is a computer; written by anyone but
root, it's refused.

### `/etc/lnk/users/<name>/`

Mode 755, root's: [a user of the box](on-a-box.md#a-boxs-users) (`lnk box user add`): `user.toml`
(their folder and key), `<name>.toml` (their sandbox's specification, its paths
absolute) and `authorized_keys` (their key's one line, a forced command
into their sandbox), each 644.

### `/usr/local/lib/lnk/`

Mode 755, root's: root's copies of `lnk` and `lnk-sandbox`, which a box
user's sessions run, refreshed by each `lnk box user add`. Ordinary
workers also enter through `lnk-sandbox`;
[Worker Controller Maintenance](../../../../../../security/sandbox/docs/security/sandbox/more/workers.md)
explains refreshing that helper after sandbox upgrades.

### `/etc/ssh/sshd_config.d/60-lnk-users.conf`

Mode 644, root's: for the group `lnk-users`, their keys read from
`/etc/lnk/users/<name>/authorized_keys`, never their own home's, and no
forwarding.

## A Linux box's workers

What the [worker installer](../../../../../../security/sandbox/docs/security/sandbox/more/workers.md)
puts on a Linux box for its [working users](on-a-box.md#working-users).

### `/etc/lnk/workers.json`

Mode 644, root's: the worker launch mode. `ordinary` records the
controller's `uid`, `gid` and `groups`; `packed` selects the image's
existing slot identities. The worker installer selects ordinary mode. A box without these trusted settings refuses work.

### `/var/lib/lnk/workers/`

Mode 700, root's: worker account journals and the durable monotonic
`next-id` counter. Preserve the counter across upgrades; missing or
invalid state refuses new work.

### `/run/lnk-workers/<name>/`

Mode 700, root's: a worker's temporary private mount views, detached
during cleanup without removing the underlying state.

### `/etc/sudoers.d/lnk-worker-descriptors`

Mode 440, root's: command-scoped descriptor and PATH handling for the
installed helper; it grants no sudo authorization.

### `/etc/systemd/system/lnk-workers-reap.service`

Mode 600, root's: boot cleanup of interrupted worker journals, installed
by the worker installer.

## In the cloud

What Link names and tags in the cloud account, and what it keeps there
rather than on your computer.

### A box's machine

Each box's machine is named `lnk-<installation id>` and tagged (labelled on
Google Cloud) `lnk-box=<name>`, `lnk-machine=<installation id>` and
`lnk-owner=<your account's owner ID>`. A specification's own tags are added to
those, at most 40; on AWS its disk has them too, and a spot box's
request.

### Its host keys

The host keys pinned at its first boot are kept on it: metadata
`lnk-host-keys` on Google Cloud, a `lnk-host-key` tag on AWS.

### An image

An image `lnk box image create` makes is named `lnk-<name>` in the box's
account and tagged (labelled) `lnk-image=<name>` and `lnk-owner=<your
account's owner ID>`, and `lnk-hash=<hash>` when saved with `--hash`;
on AWS its snapshot has them too. Nothing about images is kept on your
computer.

## Command Environment

The box commands supply context to programs they run. Test overrides select
stand-ins for external programs and should be confined to isolated fixtures.

### `LNK_BOX_HERE`

A command started by `lnk box run` or `lnk box ssh` in a box here receives
this box's name. Box-user commands refuse that context because users require
a machine of their own.

### External Programs

`LNK_AWS` and `LNK_GCLOUD` select cloud CLI stand-ins for tests. `LNK_SSH` and
`LNK_SSH_KEYGEN` select the SSH client and certificate utility the box commands
run. An ordinary installation uses the discovered programs instead.

### `LNK_TEST_BOX_END`

A debug build reads the disposable-box ending policy from this file instead
of `/etc/lnk/end.json`. A release build ignores it, so the override cannot
replace a deployed box's ending policy.
