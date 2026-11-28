# Box Images

Which image a new box starts from, and what an image you save holds,
[refuses](../security.md) and gives each box made from it. List images saved in a connected AWS account.

```sh
lnk box image list aws
```

## A box's image

Each OS is an image from a fixed table, found by its publisher, never
by searching image names, which anyone can publish under: Canonical's
SSM parameters on AWS and its `ubuntu-os-cloud` project on Google
Cloud. A box with GPUs gets NVIDIA's driver from AWS's Deep Learning
Base AMI (AWS's own SSM parameter) or Canonical's
`ubuntu-os-accelerator-images` project.

## Images you save

`lnk box image create` keeps a box's disk as an image in its cloud
account, for `lnk box create --image`.

### What it holds

Everything on the box's disk is copied into every box made from it. Its
disk alone: a volume attached to the box (`[attach] disks`) stays out
of the image, on AWS as on Google Cloud, so no box made from it holds or
keeps a copy. Image creation refuses credentials reported by any
installed module.
Remove them through their owning module before saving.

### Deleted files' blocks

After credentials are removed, `fstrim -a` (with `sudo`) drops
the disk's freed blocks before it's saved, where the disk takes it; on
one that doesn't, the old file's blocks can stay in the image until
written over.

### Fails closed

Every check on the box (its agents, its secrets, its login) refuses the
save when the box's answer fails or isn't what `lnk` prints: an SSH
error is never taken for "none".

### Consistent

The box is stopped while its disk is saved, so no file is saved
half-written, and started again after.

### Cost

The question before saving says the image is billed every month by its
size. [`lnk uninstall`](../../../../../../../docs/more/modules/guide.md#uninstall) deletes no image: it says they stay in your
accounts.

## What saving refuses

A box holding secrets where they're usually left, Link's own stores,
agents, or users of its own is refused, and nothing on it is removed.

### Secrets where they're usually left

Before it changes anything, `save` runs a script over SSH that looks in
`ubuntu`'s home folder, and refuses the box, listing what it found:
`~/.git-credentials`, `~/.config/git/credentials`, a `~/.netrc` with a
password, the GitHub CLI's login, an SSH private key (`~/.ssh/id_*` but
`.pub`), and the AWS, gcloud, Azure, Docker and Kubernetes logins.

### Package registries' tokens

A token in an `.npmrc`, `.yarnrc.yml` or `.pypirc` in the home folder or
up to three folders down (but in `node_modules`, `.git` and `.venv`)
refuses it too; one written as `${VARIABLE}` is read from the
environment, and isn't one.

### Link's own, as each module says

It asks the box's `lnk` what each of Link's modules there keeps (`lnk
uninstall --parts --json`, each module's own answer), and refuses the
box when any part's secret paths hold anything: a folder with anything
in it, or a file that isn't empty. A part's secret paths are those it
names, else all of its paths, so a module that says nothing more is
checked whole. Only Link's own modules are asked, as `lnk uninstall`
asks them.

### What that refuses

[`lnk cloud`](../../../../../../cloud/cloud/docs/services/cloud/README.md#platforms)'s, `lnk box`'s and each cloud adapter's folders (accounts,
logins, the SSH key), [`lnk storage`](../../../../../../storage/storage/docs/services/storage/README.md#storage)'s (keys and encryption passwords)
and `~/Link/Files`, the agents' folders, settings and notes
(`~/Link/Agents`, `~/.lnk/data/agents/move`) are refused wherever their
module keeps them; [`lnk measure`](../../../../../../tooling/measure/docs/tooling/measure/README.md#measure)'s numbers aren't secrets. Its login
(`relay`) is signed out instead, above.

### A path it can't read

A box whose `lnk` doesn't answer is refused, and
so is a path that isn't absolute or holds a control character. A secret
path the box's user can't look into (a folder above it, or itself, not
its to search or read) is taken as holding something, so a store a part
keeps in another user's home, as a keeper of secrets does, refuses the
box rather than passing unseen.

### No agents

A box with agents on it is refused, since each box from the image would
run them as themselves, with their keys and connections.

### No users

A box with [users of its own](../user/README.md) is refused, naming
them: each member of the `lnk-users` group, and each folder in `/home`
but `ubuntu`'s, a removed user's kept folder among them. The secret scan
reads `ubuntu`'s home alone, so an image would otherwise carry their
homes, Safes, keys and secrets to every box made from it.

### It removes nothing

The box is yours, and still in use. A secret anywhere else, under
another name, or in another user's home (`root`'s) isn't looked for.

### Against a mistake

It's a check the box makes of itself, against a mistake, not a defence
against the box's root: whatever root mounts over a store's path (an
empty folder bound over it) hides the store from the check, and the
image keeps what's under the mount.

## A box from an image

What a box made from an image you saved keeps of it, and what it gets
of its own.

### Its own identity

A box from an image receives its own installation identity.
Before `lnk` is set up there, the saved box's installation ID is deleted, and
the box gets an ID of its own.

### The image's `lnk`

A box from an image runs the `lnk` this computer checked: one of another
version is replaced by the checked `install.sh`, and the new `lnk`
brings the image's modules to its version ([`lnk upgrade`](../../../../../../../docs/more/modules/guide.md#upgrade), with its own
keys).

### The image's SSH keys

The box keeps the image's `~/.ssh/authorized_keys`: a key removed from
boxes after the image was saved still opens every box made from it.
Save the image again after removing a key.

### Its own host keys

Ubuntu's cloud-init deletes an image's SSH host keys and makes new ones
at the first boot of each new machine (`ssh_deletekeys`, run once per
instance), and prints them on its console, where `lnk box create` reads
them as for any box.

### Never the saved box's

They're pinned under the new box's own name (`lnk-<cloud>-<its id>`),
so the saved box's keys are never trusted for it. Nothing Link keeps on
the saved box's instance (its host keys, its tags) is in the image.

### Yours only

An image is tagged `lnk-image=<name>` and `lnk-owner=acct-<id>` (on AWS
its snapshot too). `lnk box image list` lists only yours. A box is made
only from the account's own image (`--owners self` on AWS, the
project's own on Google Cloud), never a public one by the same name.
`lnk box image delete` deletes an image only when it's tagged as Link's
for you.

### Hashes

An image saved with `--hash` is tagged `lnk-hash=<hash>` too. The hash
is yours, made from what you choose; Link checks only that it's a tag's
lowercase letters, digits and dashes. `lnk box create --image-hash`
takes the newest ready image of yours with that tag, as `--image` takes
one by name: the tag is as trusted as the account, where anyone who can
tag images could make one answer a hash.
