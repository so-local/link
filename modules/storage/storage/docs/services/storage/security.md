# Security

What [buckets](README.md) protect: your files and their copies in your
own clouds, against losing the last good copy, and the keys to those
clouds. Files go straight between this machine and your clouds, and
buckets use your own cloud accounts and bills: Local Link holds no
storage for you.

## Cloud keys

Where the keys to your clouds are kept, how they're asked for, and how
each rclone run is handed the one it needs.

### `rclone.conf`

Keys are in `~/.lnk/safe/storage/rclone.conf`: mode 600, in a 700 folder.
Link's own edits are atomic and made under a lock. rclone rewrites the
file itself when a sign-in token refreshes.

### Secrets at a prompt

Secrets are typed at a prompt with echo off. `connect <type> key=value`
refuses any setting rclone marks secret or sensitive, such as passwords,
keys, account ids and hosts; rclone then asks for them.

### AWS and Google Cloud

On AWS and Google Cloud, a bucket signs in as its [`lnk cloud`](../../../../../cloud/cloud/docs/services/cloud/README.md#platforms) [account
and nothing else](../../../../../compute/box/docs/services/boxes/security.md#keys).
`rclone.conf` holds none of that sign-in. Each rclone run on the bucket
gets the account's from `lnk cloud`: its aws profile's name or its
access key, or its Google credential file, as
`RCLONE_CONFIG_<SECTION>_<KEY>` variables, or as
`GOOGLE_APPLICATION_CREDENTIALS` for a person's Google login.

#### The shell's own keys

For those runs the shell's own `AWS_*` and
`GOOGLE_APPLICATION_CREDENTIALS` are removed, so the account used is the
one connected, never whatever the shell says at that moment.

### Encryption passwords

Encryption passwords are kept in the same file, obscured. That is not
encryption: the file's permissions protect it. It's the only stored
copy. [`lnk uninstall`](../../../../../../docs/more/modules/guide.md#uninstall) keeps it unless you pick `buckets`, and says so.

### On a Mac

On a Mac, the cloud keys and encryption passwords Link writes are in the
[Keychain](../../../../../../docs/more/architecture/security.md#secrets-in-the-keychain) instead,
and `rclone-keychain.toml` beside `rclone.conf` lists which.

#### How a run gets them

Each `lnk storage` command reads them from the Keychain once, holds them
in memory while it runs, and passes them to rclone in its environment
(`RCLONE_CONFIG_<SECTION>_<KEY>`) for each run. A sign-in rclone saves
itself, such as a Google Drive token, stays in the file.

### Other storage

Other storage (`connect b2`, `drive`, `sftp`, ...) is set up by rclone's
own questions; its keys or sign-in tokens go in the same settings file.
`lnk storage connect` refuses rclone types that wrap other remotes (`crypt`,
`alias`, `union`, ...) or aren't storage (`local`, `http`, `memory`).

### A run's own settings file

For every command, and for another module's objects kept in an owner's
bucket through `lnk storage`'s engine, rclone is handed, for each run, a settings file of its own (mode 600, `~/.lnk/safe/storage/runs/`) holding
only the sections that run reaches, its secrets from the Keychain too,
never any of them in its environment. It's read back and removed once
the run is done; one a killed run left is removed at the next start,
once its process is gone (and any after a day).

## Files Link keeps

All in `~/.lnk/safe/storage/`, each named in
[Files](files.md):

- `clouds.toml`: which clouds are connected, and which bucket is which
  folder's.
- `rclone.conf`: cloud keys and encryption passwords (see above).
- `manifest-<personal or kind-id>.json`: what went where, each file's
  size and modification time, and when each copy was last checked.
- `engine`: the pinned rclone.

### Temporary files and folders

Each rclone run gets its file list in a temporary file, mode 600 and
created new, deleted afterwards. A drive cloud's backup folder is
created 700.

## The engine

rclone at a pinned version, downloaded over HTTPS from its GitHub
releases and installed only if its SHA-256 matches the one in the
module's source. Archives over 100 MB are refused.

### Its pinned sums

The pinned sums come from rclone's `SHA256SUMS`, checked against its
release signer's PGP signature when pinned.

### How it runs

It runs with its own settings file and every `RCLONE_*` variable removed.
File names are always passed to rclone after `--` or in a file list,
never where rclone would read them as options.

### `LNK_RCLONE`

`LNK_RCLONE` points at another rclone, for tests. Whoever can set it can
already run programs as the user.

## Keeping a good copy

What `clean` proves before it deletes, and what `push` does and doesn't
check.

### What `clean` guarantees

At the moment it runs, every connected cloud has a copy of each file
that matches the file here, and every file was reported on. The check is
MD5 on S3 and GCS, through the encryption for encrypted clouds, or a
byte-for-byte read where the copies have no checksum, encrypted or not.
That catches missing, truncated and corrupted copies.

#### On S3, a checksum the uploader set is not taken

S3 computes an object's MD5 only for an upload in one part, in its
ETag. For an upload in parts, rclone takes the MD5 from the object's
`md5chksum` metadata, which whoever uploaded it sets and S3 never
checks. Anyone who can write to the bucket could replace a file with
junk carrying the original's MD5, a box with an owner's bucket's key
included.

#### So it reads them back

On aws, s3 and rclone `s3` clouds, `clean` first asks for each object's
metadata (one request per file) and reads back every copy that carries
an MD5 there, through the encryption too.

#### A copy is proved once

On those clouds, what a `clean` proved (its dry run included) is kept
in the manifest: the file here, by its size, inode, and modification
and change times to the nanosecond (a write here that puts the
modification time back still moves the change time), and its copy, by
its size and the time S3 wrote it, from a listing that reads no file.

#### Why S3's time

Only S3 sets that time, so a copy written since in one request shows
another and is checked again. An upload in parts takes the second it
began, so junk of the copy's size finished after a `clean` from an
upload begun the second the copy was written would list as the copy
does.

#### Uploads not finished

So before it checks, `clean` asks S3 for the uploads in parts begun and
not finished (`rclone backend list-multipart-uploads`): a copy with one
on its name, or written in the second the check began, or on a cloud
that can't say, is checked but not proved. One finished before the
check is read back as the junk it is, and one begun after it begins in
a later second than the copy's.

#### A later `clean`

A later `clean` finding both as they were takes the copy as proved,
without asking for its metadata or reading it back. The first check of a
copy still asks for its metadata, one request per file, 32 at a time:
rclone shows an object's ETag only through it.

#### A copy written during the check isn't taken

The copies are listed again after a check that checked any; one written
in between, to swap a checked copy for junk carrying its MD5, is
reported as written while it was checked, and nothing is deleted.

#### Links and overlapping folders

A "copy" in a drive cloud that is the file itself is refused: the
backup folder being a link back into Link's folders, or a hard link.
Every command refuses a drive cloud whose backup folder has come to
overlap Link's folders. A file changed or replaced after the check, or
now reached through a link, is kept.

### What reading back costs

Reading back costs what a download does, and takes as long, once per
copy: a dry run then a `clean` reads it once. rclone sends a file over
200 MiB in parts, with its MD5 in that metadata, so those are the copies
read back. An archived copy can't be read until it's restored, so
`clean` keeps such a file here.

### What `push` takes

`push` takes the checksum the cloud reports, so it doesn't notice a copy
replaced this way and doesn't send the file again, `--replace` included:
reading every copy back on every push would cost a download each time.
`clean` finds it and says it differs. To send the file again regardless
of checksums:

```sh
lnk storage push photos/2019 --recheck --replace -- --ignore-times
```

### What no check protects against

A cloud losing data later, a closed cloud account, lost keys or a lost
encryption password, or a cloud that sets out to deceive: the provider
writes the ETag too, and MD5 is not collision-resistant against an
attacker. On other storage rclone reaches, such as Backblaze B2 or
Azure, the checksum of a large upload can also be the uploader's word,
and `clean` takes it.

### Local files

What keeps a bad file here from replacing a good copy, and a file from
being taken for another.

#### Bad local files can't overwrite good copies

`push` refuses to replace a copy that differs unless `--replace`, so
files corrupted or encrypted by ransomware here are reported instead of
sent. Files never pushed are only as safe as this machine.

#### `push` trusts size and modification time

A file with the same size and modification time, to the nanosecond
where the file system keeps them, as when its copy was checked is
skipped unread. A file changed without either changing, or a copy
damaged in the cloud since, goes unnoticed by `push`; `push --recheck` and
`clean` check every file.

#### Names clouds store differently are skipped

A file whose name has control characters, or the look-alike characters
rclone uses for them, is never pushed or cleaned, so two such files
can't be taken for one another.

#### Links stay inside the folders

`push` and `clean` skip symlinks. `pull` refuses any file whose way from
the folder goes through a link, which could lead outside it, for
example into `~/Library/LaunchAgents`.

#### The folders are kept apart

A command that touches files refuses to work when your files' folder
and an owner's are one inside the other, or when either is or holds
your home folder or Link's settings. A module that keeps owners but
can't list their folders no longer stops it. Its folders are [left out
with a warning](#when-a-module-doesnt-answer).

### Deleting from clouds

`remove` counts a copy in another cloud as kept only if a listing of
that cloud, made now, has it at its recorded size.

## Who can read the files

Who reads your files in a cloud depends on `--encrypt`, and what a
bucket's name and labels say of you.

### With and without `--encrypt`

Without `--encrypt`, the cloud provider, and anyone with access to the
bucket. Buckets Link makes are private, GCS ones with uniform
bucket-level access. With `--encrypt`, rclone's `crypt` encrypts
contents (XSalsa20-Poly1305) and names before upload.

### Bucket names say nothing about you

`lnk-bucket-` and 12 random characters, since bucket names are global
and anyone can test whether one exists.

### Labels do

An owner's bucket, in a cloud from `lnk cloud`, is labelled
`lnk-folder` (its owner's ID, name-safe), `lnk-folder-name` (its owner's
name) and `lnk-owner` (your account's owner ID, `acct-<id>`). Anyone who
can list the account's buckets can read them, as with a box's tags.

#### `lnk-layout`

A backup adds the `lnk-layout` label through the
[storage interface](more/protocol.md#lnk-storage-label---owner-kindid-namevalue), which a restore reads
to know how the backup is laid out: only your account sets it, since
the key a box gets for an owner's bucket reaches its objects alone
(aws's `s3:GetObject`, `PutObject` and the like, gcp's
`roles/storage.objectAdmin`), never its labels.

## Owners' folders

A folder another module says is its own (`storage.folders`) is an
owner's, [its owner named by a stable ID](../../../contract/docs/services/storage/more/contract.md#owners), `<kind>:<id>`.
`lnk storage` treats the kind as data, and keeps only the ID: a person's
`--owner <kind>:<name>` is resolved by the module that keeps the kind
(`storage.owner`). Each owner's folder has its own bucket, always
encrypted, apart from yours. Link backs both up with the key you
connected, which reaches both.

### When a module doesn't answer

The commands that only read or change settings never ask which folders
are owners'. These are `list`, `show`, `create`, `connect`,
`disconnect` and `delete`, which read each bucket from `clouds.toml` by
its folder's key. Only `connect --owner-bucket` without `--owner` asks,
to find the one owner here, and refuses when a module doesn't answer.

#### Commands that touch files

The commands that touch files, `push`, `pull`, `tree`, `clean` and
`remove`, ask every module that answers `storage.folders`. One that
fails doesn't stop them. Its folders are left out, with a warning that
names the module and ends with what it last said on stderr. A path in
one of its folders is then in none of Link's folders, and refused. An
owner's folder is reached only while its own module answers, as before.

#### An owner's folder inside yours

That leaves one gap. While a module doesn't answer, `lnk storage` can't
tell whether one of its folders is inside your files' folder, [which it
refuses whenever it can tell](#the-folders-are-kept-apart). A push of your files could then
send that folder's files to your bucket, and `clean` could delete them
here once every cloud has a checked copy. Fix the module before pushing
or cleaning a folder that holds an owner's.

#### Settings commands without owners' folders

`disconnect` and `delete` count an owner's file as having its only copy
in the cloud whenever no other cloud has one, since where its folder is
goes unasked. A drive cloud is checked against your files' folder when
it's connected, and against every folder that answered before each
command that touches files.

### Its name-safe form

Where a name can't hold a `:`, an owner's ID is written `<kind>-<id>`
(`group-t4m9q2x7k1ab`): its rclone sections, its manifest, its folder
on storage with folders (`local-link/<kind>-<id>`), a cloud only for it
(`<cloud>-<kind>-<id>`) and its `lnk-folder` label. `lnk storage` takes
only an owner whose ID's id is lowercase letters and digits, and whose
form fits a label (63), so the form reads back to one ID alone.

### While it runs

`clean` and `pull` refuse in an owner's folder while its owner runs,
and when the module that keeps it can't say whether it does. `clean`
asks again right before deleting, and refuses if that module doesn't
answer then.

### Its password

Reconnecting an encrypted bucket, or taking an owner's in, needs its
password: `lnk storage connect` and `adopt` refuse one that doesn't decrypt a file
already there. A file's sealed first block decides, not its name: about
one name in 256 decrypts under a wrong password, by chance. A name
decides alone only when every file it checks is empty.

### A box's buckets

When an owner's folder moves here from a box, the box names its
buckets. Link takes only a name it gives an owner's bucket, in any
cloud: `lnk-bucket-` and 12 random characters, or
`local-link/<kind>-<id>` on a folder. In a cloud from `lnk cloud`, the
bucket must also be labelled as that owner's. So a box can't point an
owner's backups at another bucket or folder of yours.

### Found by their labels

Finding an owner's buckets by their labels takes, when you're logged
in, only buckets labelled with your login's `lnk-owner` for `lnk storage
restore`. A box's keys, revoking them and a move's buckets also take one
with no account, made before you logged in, and never one labelled as
another login's.

#### Two buckets for one owner

Labels are only what anyone who can make buckets in the account says,
so two buckets labelled as one owner's in one cloud are refused, both
named: delete the one Link didn't make.

### Objects beside its files

Another module may keep objects of its own in an owner's bucket, through
`lnk storage`'s engine, under `.link/`, which a push, a pull and a clean
of its files never touch. Names and contents are encrypted as the rest
of the bucket is.

## `lnk storage rclone` is unguarded

It runs the pinned rclone with Link's settings file, so it can do anything the
connected keys allow, including `sync` and `purge`, which delete. Link's
refusals apply only to its own commands.

## Gaps

What buckets don't protect, and a command whose meaning changed.

### Keys in a file

Outside macOS, keys and passwords are kept in private files. On macOS,
secret rclone settings go to the Keychain; Google Cloud credential files
still contain account authority. One connected account key can reach both
your bucket and owners'. See [known
limitations](../../../../../../docs/more/architecture/security.md#known-limitations).

### A module that doesn't answer

[While a module that keeps owners can't list their folders](#when-a-module-doesnt-answer), an owner's
folder inside your files' folder goes unnoticed, and is pushed and
cleaned as yours.

### Archive tiers

Archive tiers exist only on aws and gcp. Other storage classes need
`lnk storage rclone`.
