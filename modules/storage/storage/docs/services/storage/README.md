# Storage

> Storage Keeps Your Folders in Your Clouds

Storage backs up your folders to the clouds you connect, verifies each
copy, and restores files when you need them. Your cloud provider bills
for storage and transfers.

<!-- quickstart -->

## Create a Bucket

Create a bucket in your Amazon Web Services or Google Cloud account.
Local Link connects the account first if needed. You can also [connect
other providers and existing buckets](guide.md#connect-a-cloud), and
[encrypt your files](guide.md#encrypt-your-files) with `--encrypt` so only
you can read them.

```sh
lnk storage create aws|gcp
```

## Back Up a Folder

Copy a folder to every connected cloud and verify the copies. This
example uses `photos/2019` in `~/Link/Files`. You can [select other
folders](guide.md#its-paths) to back up.

```sh
lnk storage push photos/2019
```

## See What Is Stored

See which files are stored here and on each cloud, even while offline.
The guide explains [how copies are tracked and checked](guide.md#see-whats-where).

```sh
lnk storage list --tree --level 2
```

## Restore Files

Restore matching files from the cheapest cloud and verify them. The guide
covers [selecting files](guide.md#get-files-back) and [safely removing a
local copy](guide.md#free-up-space) after your backups are checked.

```sh
lnk storage pull 'photos/2019/**/*.jpg'
```

## Show a Connected Cloud

Inspect one connected cloud's configuration and what it holds. The command
never prints a credential or encryption password, and the saved records
behind it are in [Files](files.md).

```sh
lnk storage show aws|gcp
```

## Trust a Copy and Fix Failures

You learn when a copy is trusted and what encryption protects in
[Security](security.md). You fix transfer and credential failures with
[Troubleshooting](troubleshooting.md), and you read why storage and
encryption work as they do in [Decisions](decisions.md).

## Build on Storage

Programs reach storage as the [Interface](more/protocol.md) documents.
The [Storage Contract](../../../contract/docs/services/storage/more/contract.md)
defines owner folders and the typed requests that connect storage to
other modules.

<!-- docs-tree:start -->

## Docs

### Pages

- [Storage Guide](guide.md): Your folders, in your clouds
- [Files](files.md): The files `lnk storage` keeps in `~/.lnk/safe/storage/`, and what each one holds

### More

- [Interface](more/protocol.md): What other programs can rely on from `lnk storage`
- [Decisions](decisions.md): Why buckets work the way they do
- [Security](security.md): What buckets protect: your files and their copies in your own clouds, against losing the last good copy, and the keys to those clouds
- [Troubleshooting](troubleshooting.md): What to do when [a `lnk storage` command](README.md) refuses or misses files

<!-- docs-tree:end -->
