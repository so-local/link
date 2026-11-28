# Storage Guide

**Your folders, in your clouds.** Back up your files to every cloud you
connect, check every copy, and free the space here once they're safe.

```sh
lnk up
lnk storage create aws
lnk storage push photos/2019
lnk storage list --tree --level 2
lnk storage clean photos/2019
lnk storage pull 'photos/2019/**/*.jpg'
```

## Its paths

Paths are in `~/Link/Files` or an [owner's folder](#back-up-an-owners-folder). A relative path starts from the current
folder when you're in one of them, and from `~/Link/Files` otherwise.
`pull` and `remove` take globs, so quote them and use `**` to
span folders.

## Connect a cloud

Connect as many clouds as you like, and `push` copies to every one.

```sh
lnk storage create aws --profile work
lnk storage create gcp
lnk storage create gcp --cloud gcp-ci
lnk storage create s3 --endpoint https://0123abcd.r2.cloudflarestorage.com --key-id AKIAEXAMPLE
lnk storage create drive --path /Volumes/MyDrive
lnk storage create b2
lnk storage list
```

### Another rclone storage type

Any storage type rclone knows, such as `b2` for Backblaze B2, connects
by its type. `--setting KEY=VALUE` gives one of that type's own
settings, once for each, and rclone asks for the rest, its secrets among
them, so none is typed on the command line.

```sh
lnk storage create b2 --setting hard_delete=true
```

### What `lnk storage create` makes

`lnk storage create` makes a private bucket for your files, once per cloud, and
tests the keys. If a test fails after the bucket is made, it deletes the
bucket again, or says which one it couldn't.

### Say yes to a bucket

The cloud bills a bucket for what it stores, so Link asks before making
one, at `lnk storage create` and at the first `lnk storage push` that
needs one. `--yes` answers yes ahead. With no terminal and no `--yes`,
the command makes nothing and names `--yes`. A `drive` cloud, or an
rclone type without buckets, gets a folder, which no cloud bills, so
nothing is asked of it.

```sh
lnk storage create gcp --yes
```

### See your clouds

`lnk storage list` shows a section for each cloud, headed by its name,
type and where it is. Its table has a row for each folder it holds,
your files or an owner's, with the bucket, its encryption, and how many
files it holds and their size. `lnk storage show <name>` shows one
cloud: its name, type and where it is, then a section for each folder
it holds. Each section has the bucket and its ID, its encryption, where
its password is kept, its name in rclone, its files and size, and when
a copy there last matched the file here. Both [print JSON only with
`--json`](more/protocol.md#lnk-storage-list---json).

```sh
lnk storage list
lnk storage show aws
```

### AWS and Google Cloud

On AWS and Google Cloud a storage cloud is a cloud account from
[`lnk cloud connect`](../../../../../cloud/cloud/docs/services/cloud/guide.md#connect-a-cloud-account),
the same one your boxes there use, and is named as it. `--cloud` picks
the account, by default the one named `aws` or `gcp`. `lnk storage
create` connects it first when there's none, asking what `lnk cloud
connect` asks. A service account connects there, with its key: `lnk
cloud connect gcp --name gcp-ci --credential-file key.json`. Every
other storage is a remote, named by `--name`, which `--remote` picks
in `push`, `pull` and `remove`.

### Name the bucket

`lnk storage create` names each bucket it makes `lnk-bucket-` and 12
random letters and digits, and [takes no name of yours](decisions.md#why-are-bucket-names-random).
To use a name you chose, make the bucket in your cloud first, then
connect it. On Google Cloud, `gcloud storage buckets create
gs://my-files-0123 --location US` makes one, and `lnk storage connect`
adds it.

```sh
lnk storage connect gcp --bucket my-files-0123
```

A name is 3 to 63 lowercase letters, digits, `-` and `.`, starting and
ending with a letter or digit, and unique across the whole cloud.
`--region` and, on Google Cloud, `--project` say where `lnk storage
create` makes a bucket.

## Encrypt your files

With `--encrypt`, your files and their names are encrypted on this
machine before they're sent, so your cloud provider and anyone else
with access to the bucket hold only unreadable data. Only a machine
with the bucket's encryption password reads them back.

### What encrypts them

Link encrypts with [rclone's `crypt`](decisions.md#why-rclones-crypt-for-encryption), the encryption layer of the tool
that moves the files.
Each file's contents are sealed with XSalsa20-Poly1305, which both
hides them and detects any change, and each name in its path is
encrypted too. File sizes and modification times stay visible to the
cloud.

### Choose it when the bucket is made

Encryption is set per bucket when you create or connect it, and stays
as it was set. A bucket that already holds files without encryption
can't be switched over: connecting it with `--encrypt` is refused,
because no password opens the files already there. Instead, disconnect
that cloud with `lnk storage disconnect`, which leaves its bucket as it
is, create a new encrypted bucket and push your files again.

### Create an encrypted bucket

`lnk storage create` with `--encrypt` makes the bucket and its
encryption password, and shows the password once.

```sh
lnk storage create gcp --encrypt
```

The password is 32 random letters and digits that Link makes. Save it
in a password manager while it's on screen. Once this machine's copy
is gone, nobody can read the files without it, Link included.

### Encrypt a bucket you named

Connecting a bucket you made yourself with `--encrypt` asks for a
password, which you choose. Type it at the hidden prompt and keep it,
since every other machine needs it.

```sh
lnk storage connect gcp --bucket my-files-0123 --encrypt
```

The bucket should be empty, since a bucket holding files without
encryption is refused.

### Where the password is kept

Link keeps the password with the cloud's other settings, readable only
by you. On a Mac it's in the Keychain, and elsewhere in
`~/.lnk/safe/storage/rclone.conf`, [obscured but not encrypted](security.md#encryption-passwords). No `lnk storage`
command shows it again, and `lnk storage show` names only where it is,
never the password itself.
Every `push`, `pull` and `tree` reads it from there, so you type it
only when connecting.

### Reach it from another machine

On another machine, connect the same bucket with `--encrypt`, and type
the password when asked. `lnk storage list` on the first machine shows
the bucket's name.

```sh
lnk storage connect gcp --bucket lnk-bucket-a1b2c3d4e5f6 --encrypt
```

Link [checks the password](security.md#its-password) by decrypting a file already in the bucket and
refuses one that doesn't open it. The prompt needs a terminal,
so connect each machine by hand once, and its later commands read the
saved copy.

### Reach it from an agent on a box

An agent's files go to [buckets of their own](#back-up-an-owners-folder), always encrypted with one
password for that agent, apart from your files' bucket. When you move
the agent to a box in Google Cloud with [`lnk agent move gcp`](../../../../../agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud),
its buckets and their password go with it. The box gets a Google Cloud
service account, made through your account, whose key reaches only
that agent's buckets, so a push of the agent's files works there.

```sh
lnk agent move gcp --agent work
```

The box holds none of your cloud accounts, so an agent there never
reaches your own files' bucket.

## Back up

`push` sends files to every cloud at once and checks every copy it sent.
It skips files unchanged since their copy was checked, never deletes
anything in a cloud, and never replaces a copy that differs from the
file here unless you pass `--replace`.

```sh
lnk storage push photos/2019
lnk storage push photos --cloud aws -- --bwlimit 10M --transfers 8
```

### Back up an owner's folder

Another module can say a folder here is one it keeps, such as a group's
shared folder: an owner's folder, [its owner named by a stable ID](../../../contract/docs/services/storage/more/contract.md#owners),
`<kind>:<id>`.
`lnk storage` never takes it for your files. Each goes to a bucket of its
own in every cloud, made on its first push [after your yes](#say-yes-to-a-bucket) and always encrypted, with
one password per owner. The same commands work on it, but `pull` and
`clean` refuse while its owner runs, or when the module that keeps it
can't tell, so stop the owner first.

```sh
lnk storage push <folder>
lnk storage clean <folder>
```

`lnk storage list` names an owner's bucket by the owner's name, which
its module gives, such as `main`. It still lists the bucket by the
owner's ID, from Link's settings, when that module isn't here or doesn't
answer. `lnk storage show` names it by its ID.
When that module fails to say which folders it keeps, the commands that
touch files [warn and go on with your files](security.md#when-a-module-doesnt-answer), leaving its folders out.

#### When it moves

An owner's buckets move with its folder: the module that keeps it [asks
`lnk storage` for them](more/protocol.md) by the owner's ID, and puts them in place where
the folder lands. On a box, its folder backs
up with a key that reaches only its bucket. If its machine is gone, the
buckets are found again by their labels in your [`lnk cloud`](../../../../../cloud/cloud/docs/services/cloud/README.md#platforms) accounts.

#### Objects beside its files

A module may keep objects of its own in an owner's encrypted bucket,
under `.link/`, apart from its files. `lnk storage` never pushes, pulls
or lists anything under `.link/` as a file, and any machine connected to
the bucket reads those objects as they were left.

### A folder a cloud account keeps

An owner's folder may be kept on a cloud account rather than in your
buckets, as the module that keeps it answers `storage.folders`. Push and
pull reach it too, from and to its folder here, but [through that module](../../../contract/docs/services/storage/more/contract.md#storagehome)
(`storage.home`) and that account, never rclone and never a key here.

## Get files back

`pull` copies each file from the cheapest cloud that has it and checks
it. Downloading from AWS or Google Cloud costs about $90 per TB, so it
asks before an archive restore or more than 1 GB of charged downloads.
It doesn't overwrite a file here that differs, unless you pass
`--replace`.

```sh
lnk storage pull 'photos/2019/**/*.jpg'
lnk storage pull photos/2019 --cloud gcp
```

### See what's where

Each file gets one mark per cloud: `✓` checked, `A` checked in an
archive tier, `?` not checked, `≠` differs, `✗` missing. `tree` works
offline, and `--refresh` finds files pushed from another machine.

```sh
lnk storage list --tree --level 2
lnk storage list --tree photos --refresh
```

`--dirs-only` shows folders alone, like `tree -d`, and `--json` prints
the tree for a script.

```sh
lnk storage list --tree --dirs-only --level 3
lnk storage list --tree --json
```

### Use a bucket from another machine

[`lnk storage connect`](#reach-it-from-another-machine) takes the same options as `lnk storage create` but makes nothing, and
stops if the bucket isn't there. An encrypted bucket also needs
`--encrypt` and its password. `lnk storage list` on the first
machine shows each bucket's name. `--owner-bucket` adds an owner's
bucket, asking its encryption password; `--owner <kind>:<name>` says
whose, as the module that keeps that kind names it, and defaults to the
only owner with a folder here.

```sh
lnk storage connect aws --bucket lnk-bucket-a1b2c3d4e5f6
lnk storage connect drive --path /Volumes/MyDrive --bucket local-link/personal
lnk storage connect aws --bucket lnk-files-0123 --owner-bucket lnk-owned-0123 --owner group:research
```

## Free up space

`clean` checks every cloud again, now, and deletes the files here only
if every cloud has a copy with the same checksum. Otherwise it says
what's missing where. It's the only Link command that deletes your files
here.

```sh
lnk storage clean photos/2019 --dry
lnk storage clean photos/2019
```

### Archive old files

Archive tiers cost about $1 per TB a month instead of about $20, and
reading files back costs money. They exist on AWS (S3 Glacier Deep
Archive) and Google Cloud (GCS Archive), and `lnk` asks before archiving
or restoring.

```sh
lnk storage push old/scans --archive
```

#### Restores and early deletes

On AWS, `pull` requests a restore that takes up to 12 hours, so run it
again once it's done. Deleting early is charged anyway, for 180 days on
AWS and 365 on Google Cloud, and `lnk storage remove` and `lnk storage delete` count.
[`clean` works on archived files without a restore](security.md#what-clean-guarantees), except a file over
200 MiB on AWS, whose copy it reads back.

## Disconnect or delete

Nothing here deletes a file's last copy unless you pass `--everywhere`.

### Remove from your clouds

`remove` deletes the copies in every cloud, or only in the cloud
accounts you give with `--cloud` and the remotes you give with
`--remote`, and keeps the files here. It takes only files and
folders, and refuses a cloud's name.

```sh
lnk storage remove old/drafts --cloud gcp
```

#### A cloud that's full

A cloud whose account reads only (past what it may keep) takes nothing
more: `push` says so before sending anything, with the cloud's own
words on making room, and `remove` asks the cloud to delete the copies
itself, so you can free space with the same command.

### Disconnect or delete a cloud

`lnk storage disconnect` prints the `lnk storage connect` command that brings the
cloud back, and asks only when the cloud holds a file's only copy.
`lnk storage delete` deletes the buckets `lnk storage create` made, your
files' and owners', and on a drive cloud Link's folders in it, each
after revoking any credential made to reach only it, so none outlives
its bucket. It asks first, and refuses a path.

```sh
lnk storage disconnect gcp
lnk storage delete aws
```

## Use rclone directly

This is rclone itself, with each connected cloud as a remote named after
it, for what `lnk storage` doesn't do, such as `mount`, `sync` and
`serve`. Link's safety checks don't apply here, and `sync` deletes. Run
`lnk storage list --tree --refresh` afterwards to catch up.

```sh
lnk storage rclone lsd aws:
lnk storage rclone mount aws: ~/mnt
```

## Where your settings live

Everything is in `~/.lnk/safe/storage/`: which clouds are connected, their
keys and encryption passwords, and what went where. On a Mac, the keys
and passwords Link writes are in the Keychain instead.

### Removing it

[`lnk mod remove storage`](../../../../../../docs/more/modules/guide.md#add-or-remove-a-module) and [`lnk uninstall`](../../../../../../docs/more/modules/guide.md#uninstall) keep this folder, and
neither deletes anything in your clouds. `lnk uninstall --delete
buckets` deletes it, along with the only copy of the encryption
passwords Link made, so save them first.

## More about it

Where the rest of what `lnk storage` does is written:

- Every flag is in `lnk storage <command> --help`.
- [Security](security.md) has what's protected and what isn't.
- [Decisions](decisions.md) has why it works this way.
- [Troubleshooting](troubleshooting.md) has what to do when a command
  refuses.
