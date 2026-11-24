# Storage Contract

`link-storage-contract` holds the three questions `lnk storage` asks the
modules built on it, as Link's answers carry them (`link_core::answers`):
which folders here are an owner's, not yours, an owned folder a cloud
account keeps pushed or pulled, and an owner's ID by its kind and name.
A module that keeps owners answers them; `lnk storage` needs none of
them to back up your own files.

## Owners

An owner is whoever a folder belongs to when it isn't yours: something
another module keeps, such as a group's shared folder. It's named by a
stable ID, `<kind>:<id>` (`group:t4m9q2x7k1ab`), on the one generic of
[Stable IDs](../../../../../../../../packages/id/docs/more/libraries/types/id.md).
The type is `Owner`, its kind `OwnerKind`, which is open: the module that
keeps a kind names it, and [`lnk storage`](../../../../../docs/services/storage/README.md#storage) treats it as data, so it never
needs to know what a kind means. Each is written as its string, in JSON
and TOML alike, and a malformed one is refused when it's read.

## `storage.folders`

`FOLDERS` asks, with nothing, for the folders here that are an owner's,
so `lnk storage` never takes one for your files, and backs each up as
that owner's. Its answer is `Folders`, `{"owned"}`, a list, and a key it
doesn't know is refused.

### Each owned folder

Each entry is `Owned`, `{"owner", "name", "root", "running"?,
"account"?}`:

- **`owner`:** the owner's ID, `<kind>:<id>`.
- **`name`:** the owner's name, to show.
- **`root`:** its folder here, absolute.
- **`running`:** whether the owner runs now, and so could change its
  files; `null` or missing when that can't be said, which `lnk storage`
  treats as running.
- **`account`:** the cloud account that keeps the folder, when it's kept
  there rather than in your buckets. Its pushes and pulls go through
  `storage.home`.

## `storage.home`

`HOME` is [`lnk storage push`](../../../../../docs/services/storage/guide.md#back-up) and `pull` for an owned folder a cloud
account keeps: pushed from, or pulled to, its folder here, through that
account. What the answering module says on stderr is shown as it comes.

### The question

`{"way", "owned", "paths"?}`: `way` is `push` or `pull`, `owned` the
folder's `Owned` with its `account`, and `paths` what to push (relative
to its folder here) or what to pull (patterns), none for all of it.

### The answer

`Moved`, `{"sent", "same"}`: the files that went, and how many the other
end held the same already.

## `storage.owner`

`OWNER` turns `--owner <kind>:<name>`, the way a person names an owner,
into the owner's ID, so `lnk storage` keeps only the ID and a rename
never moves a bucket. It's asked with `OwnerName`, `{"kind", "name"}`,
and the module that keeps the kind answers with `Found`, one of:

- **`{"here": {"owner", "name"}}`:** here, by its ID, with its name.
- **`"unstarted"`:** a kind it keeps, with that name, but no ID yet.
- **`"none"`:** a kind it keeps, but nothing of that name.
- **`"unkept"`:** not a kind it keeps.

The answering module takes its ID's id in place of a name. A program
that already holds an owner's ID passes it to `lnk storage`'s hidden
commands directly, and never asks.
