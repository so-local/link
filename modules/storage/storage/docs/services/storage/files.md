# Files

The files `lnk storage` keeps in `~/.lnk/safe/storage/`, and what each one
holds. `lnk help <command>` lists the environment variables each command
reads, as [`module.toml`](../../../module.toml) declares them. Inspect the connected clouds.

```sh
lnk storage list
```

## Clouds and their keys

Which clouds are connected, and the keys and passwords that reach them.

### `rclone.conf` (600)

Each cloud's settings and encryption passwords, and the keys of those
that don't sign in as a [`lnk cloud`](../../../../../cloud/cloud/docs/services/cloud/README.md#platforms) account: s3, rclone types, and a
box's key for one owner's bucket.

### `clouds.toml` and `rclone-keychain.toml`

`clouds.toml` holds the connected clouds and each folder's bucket, by
the folder's key: `personal`, or its owner's ID. Each bucket keeps its
name and its ID (`id`), minted as a new Link ID when Link adds the
bucket and written `bucket:<id>` in `lnk storage list --json`.
On a
Mac, `rclone-keychain.toml` holds which settings of which section of
`rclone.conf` are in the Keychain. It names its schema, and lists each
section under `[sections]`, so a section named `schema` is one like any
other:

```text
schema = 1

[sections]
aws = ["secret_access_key"]
```

## What went where

What `lnk storage` keeps of each push and check.

### `manifest-personal.json` and `manifest-<kind>-<id>.json` (600)

What went where: a cache `tree` reads, and what `clean` proved of S3
copies, so it reads each back once. [An owner's manifest](security.md#owners-folders) is named by its
ID's name-safe form, `<kind>-<id>`, since a file's name here holds no
`:`.

## Personal Files Location

`LNK_PERSONAL_DIR` selects your files folder for this process instead of
`~/Link/Files`. Its value must be an absolute path, and a relative value is
refused. It changes which personal files storage commands inspect and transfer,
so use a dedicated folder when isolating a storage fixture.
