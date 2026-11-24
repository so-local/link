# Decisions

Why [buckets](README.md) work the way they do.

## The engine

Why rclone moves the files, and how it signs in and encrypts.

### Why is rclone the engine?

rclone already reaches many clouds, and its `check` and `cryptcheck`
compare checksums, through encryption too. The module pins rclone
1.75.1, downloads it on first use, and installs it only if its SHA-256
matches the one in the module's source. It runs with its own settings file
and no `RCLONE_*` variables.

### How does a bucket sign in to AWS or Google Cloud?

As its [`lnk cloud`](../../../../../cloud/cloud/docs/services/cloud/README.md#cloud-accounts) account, the one your boxes there use: the cloud's
adapter hands rclone the account's sign-in for each command, and
`rclone.conf` keeps none of it. Buckets had signed in their own way,
which meant a second set of bugs and a service account that worked for
boxes but not buckets. Why, and how a Google Cloud account signs in:
[clouds and boxes'
decisions](../../../../../compute/box/docs/services/boxes/decisions.md#how-do-buckets-and-boxes-sign-in-to-the-same-cloud).

### Why is everything else rclone does passed through, not wrapped?

Link's commands stay the safe path, and rclone's power is one command
away. `lnk storage create` takes any rclone storage type and lets rclone ask its own
questions, `lnk storage rclone` runs rclone itself on the connected
clouds, and `push` and `pull` pass flags after `--`.

### Why rclone's `crypt` for encryption?

It encrypts file names as well as contents, and `cryptcheck` keeps
`clean` possible through the encryption. Encrypting with age before
upload would do neither. It's set per cloud at `lnk storage create`, and always on
for an owner's folder.

## Pushing and cleaning

Why `push` only adds, and what `clean` checks before it deletes.

### Why does `push` never sync deletes or renames?

A sync that deletes is how backups get lost. `push` only adds, and
replaces only with `--replace`; `remove` deletes from clouds on purpose.

### How are good copies protected from bad local files?

`push` refuses to replace a copy that differs from the file here. Link
doesn't turn on bucket versioning instead: rclone can't set it up the
same way on every cloud, and it keeps paying for old versions.

### When is a copy "checked"?

When its checksum matches the file here. Where the cloud has no
checksum, Link reads it byte for byte; through encryption, it uses
`cryptcheck`. The manifest records these checks as a cache for `tree`:
`clean` checks again every time, but for an S3 copy it read back or
proved before, unchanged here and there since.

### Why does `clean` read some S3 copies back?

On S3, rclone takes the MD5 of an object uploaded in parts from metadata
its uploader set, which S3 never checks, so anyone who can write to the
bucket could forge it. Reading every copy back would cost a full
download on every `clean`, and would never work on archived copies.
#### Only those with the uploader's MD5

Link instead asks for each object's metadata and reads back only those
carrying an MD5 there; every other S3 MD5 is the ETag, which S3
computed. rclone hides the ETag and drops that metadata from its own
listing, so Link lists with rclone's `--s3-directory-bucket`, under
which an object's MD5 shows only when it comes from the metadata. `push`
keeps comparing checksums: it is not what deletes files.

#### Proved once

What a `clean` proved is kept, so a dry run and then a `clean`, or a
`clean` tried again, don't download every large copy each time. The copy
is known by the time S3 wrote it, from a listing that costs one request
per thousand objects: S3 alone sets that time, so a copy replaced since
shows another, and is checked again. The same listing, taken again after
the check, catches a copy replaced while it ran; a `clean` that found
every copy proved checks nothing, so it lists once.

#### Where it lists

It lists only where the files are: for each path given to `clean`, the
folder its files share, so `clean photos/2019 docs` lists those two and
not the whole bucket. Past eight such folders it lists the one they all
share instead. Each cloud is listed, checked and listed again in a
thread of its own, as `push` sends.

### Why does `push` skip files unchanged since their check?

Reading every file to hash it, once to send and again to check, on every
push and for every cloud, makes a push of a large folder take as long as
the first one. `push` skips a file whose size and modification time
match the manifest and whose copy there was checked, and checks only
what it sent, like rsync without `--checksum`. `push --recheck` reads
everything, and `clean` checks every file before deleting any, taking an
S3 copy as proved only by the time S3 wrote it.

### Why does `push` send to every cloud at once?

A push to three clouds shouldn't take three times as long. Each cloud is
sent and checked in a thread of its own, with rclone's progress as a
line every few seconds after the cloud's name, since its redrawn
progress would mix. What only happens once, like a sign-in or the
Keychain, is asked first, one at a time. The manifest is written after,
one cloud after another, so no cloud's record overwrites another's.

### Why is there no copy of the manifest in each cloud?

The bucket itself is the truth. A new machine lists it (`connect
--bucket`, `tree --refresh`) and loses only the "checked" times, which
`clean` redoes regardless.

## Buckets

Why each folder has a bucket of its own, and how buckets are named, made
and removed.

### Why a bucket per folder, not one bucket with a folder per part?

Every cloud can limit a key to one bucket simply, but not to a prefix. A
bucket per folder (`~/Link/Files`, and each owner's folder) also
lets each have its own settings, so an owner's is always encrypted, and
yours holds exactly your folder. The path picks the bucket. Your files'
bucket is made at `lnk storage create`; an owner's on its first push.
Either is made only after a yes, since the cloud bills it.

### Why are bucket names random?

Bucket names are global and anyone can probe whether one exists. A new
bucket is `lnk-bucket-` plus 12 random letters and digits, with no
username or account name. On storage with folders, it's a `local-link`
folder holding `personal` and one folder per owner, by its ID's
name-safe form, `<kind>-<id>`.

### Why are `lnk storage create` and `lnk storage connect` two commands?

A command named connect that also makes a bucket surprises people. And
because `lnk storage connect` stops when the bucket isn't there, a typo in
`--bucket` never makes a new one. [`lnk cloud connect`](../../../../../cloud/cloud/docs/services/cloud/guide.md#connect-a-cloud-account) makes no bucket
either; it prints the `lnk storage create` command.

### Why does `lnk storage delete` delete everything in the buckets?

So that what Link made can be removed with Link. It still refuses,
without `--everywhere`, when a file's only copy is there. `lnk storage disconnect`
stays the way to forget a cloud and keep its files.

### Why are a cloud's buckets `lnk storage delete`, and a file's copies `remove`?

A bucket Link made goes out of existence with `delete`, and a file's
copy in a cloud is [a connection between two things](../../../../../../docs/more/developing/modules/specs.md#name-a-command), ended with
`remove`.
Each refuses the other's argument, before anything is deleted:
`lnk storage delete` refuses a path, and `remove` a connected cloud's name
that is no file of yours, and each says which command you meant.

## Owners' folders

Why `lnk storage` knows owners only by their IDs, and how an owner's
bucket is reached from a box, found from another computer, and
restored.

### Why owners, not the modules that keep them?

The storage module is built on by others, never the reverse: what
another module keeps reaches it only as [an answer to its own questions](../../../contract/docs/services/storage/more/contract.md),
in its own types. An
owner is a stable ID, `<kind>:<id>`, on the one generic every kind of
ID rides, so any module can name what it keeps, and a new kind needs no
change here. The kind is data: `lnk storage` never branches on it.

### Why is a bucket by its owner's ID, not its name?

A name can change, and two owners can share one over time; an ID never
does. A person still types a name, `--owner <kind>:<name>`, which the
module that keeps the kind turns into the ID (`storage.owner`), and only
the ID is kept. A program that holds the ID passes it directly.

### Why don't settings commands ask which folders are owners'?

A module answers `storage.folders` so a push never sweeps an owner's
folder into your bucket, and `clean` never deletes files an owner uses.
That protection matters only where files are touched. `list`, `show`,
`create`, `connect`, `disconnect` and `delete` change only
`clouds.toml`, which keys each bucket by its folder's key. Asking would
couple them to every other module, so one broken module stopped them
all, even for someone with no owners. An owner is named there by its
ID, never from a second store of names.

#### Why do file commands go on without an answer?

A module that fails is warned of and its folders left out, rather than
stopping a push, pull, tree or clean of your own files. An owner's
folder still needs its own module's answer to be reached at all.

### Why one name-safe form of an owner's ID?

An ID holds a `:`, which an rclone section, a file's name or a bucket's
label can't. One function writes `<kind>-<id>` for all of them, and
reads it back, so the same owner has the same name in each, and two
owners never share one: `lnk storage` takes only IDs whose id is
lowercase letters and digits, so the last `-` splits the form one way.

### How does a box back up an owner's folder?

With a key that reaches only its owner's bucket: an IAM user or a
service account per bucket, made through your account when the folder
arrives, and replaced or revoked when it leaves. A box holds no account
of yours, so a box broken into reaches that one bucket, still encrypted
with the owner's password.

### Why doesn't your own computer get a key per bucket?

It holds your cloud accounts themselves (`lnk cloud`), next to the
buckets' keys, so a key that reaches only one bucket wouldn't narrow
what someone who took those files could reach. A box holds no account,
so there it does.

### How does another computer find an owner's bucket?

By labels on the bucket: the owner's ID and name, and your account's
owner ID, found through `lnk cloud`. No list is kept anywhere; the
bucket is its own record, as a box is by its tags.

### How does an owner's password move with its folder?

The module that keeps the owner carries it, as rclone keeps it, from
[export to adoption](more/protocol.md#an-owners-buckets), and the
machine the folder left releases its local record. There's one password per owner, whichever cloud or
machine, and it never goes on a command line.

### Why does the storage module restore an owner's buckets?

Finding an owner's buckets by their labels, asking their password,
making a key and putting them in place is bucket knowledge. The
[storage restore interface](more/protocol.md#lnk-storage-restore---owner-kindname---folder-folder)
does that work and pulls the files before the owning module restores its
part. The caller does not build storage's adoption payload, and a change to how buckets are
found or adopted stays in the storage module.
