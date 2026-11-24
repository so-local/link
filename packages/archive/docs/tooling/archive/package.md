# Archive Package

`link-archive` reads and writes archives for every kind, knowing keys, owners and tables, never what an agent is. The archive module's [`lnk archive`](../../../../../modules/tooling/archive/docs/tooling/archive/README.md) is its one user, and the agent module writes a deleted agent's archive through it.

## What It Builds On

An archive carries a thing's state, its folders under `~/Link`, and
nothing else. The package finds them through the layout that
[`link-files`](../../../../../docs/more/reference/locations.md#state-and-prerequisites)
declares: each owner in `layout.toml`'s `[owners]` names its `kind`, the
word an archive selects its things by, and its `state` parts. The
owner's folder in each state part is a location with a stable key, such
as `link.agents` for `~/Link/Agents`, and a thing's folder there is
`<location>/<name>`.

## Selection

A selection is a kind and a name joined by a dot, `agent.main`
(`Selection`). Neither holds a dot, so it splits one way. `Pick` adds
the name an import takes, as `agent.main=agent.work`, refused when
the two kinds differ. Only the archive reads selections.

## `archive.toml`

An archive's first entry is `archive.toml`, a saved file
declared through `link-files`, so a change to its shape is
a schema step. It holds a `title` and a `description`, the few words
and the one line a person picks it by, the `title` left out of one a
person exported, the day a deletion made the
archive (`deleted`), a `[map]` of location keys to paths in the archive,
and an empty table per thing, keyed by its selection:

```toml
# archive.toml
schema = 1
title = "Programmer"
description = "A coding agent that plans, reviews and commits its own changes"

[map]
"link.agents" = "Agents"
"link.agent-settings" = "Safe/Agents"

["agent.main"]
```

The map gives each key its layout default, never this machine's path,
so where you moved your folders never reaches the archive. Import
follows whatever paths the map names, and refuses a key this Link
doesn't know, a key outside the Link folder, a path outside the archive
and two paths where one is inside the other.

### Renamed Locations

A release that renames a location renames its key in the map with a
step in the archive's manifest schema, the key quoted since it holds a dot
(`'map."link.machines"' = 'map."link.boxes"'`), so an archive made
before the rename reads under the current name.

## Writing

`Writer` writes a gzip-compressed tar file: `archive.toml` first, then
each folder's plain files and folders sorted by name. Every entry has
mode 644 or 755, time zero and no owner, so the same state writes the
same archive on any machine. Each folder and file is opened by its
folder's handle, never through a link, so a link swapped in while it's
read isn't followed. A link, a device, a file with a second name (a hard
link), a name that isn't UTF-8, or a path or size the reader wouldn't
admit refuses the export, naming it.

### Exporting

`export` checks every thing with its owner and refuses one running
before it writes anything. It writes beside the archive under a
temporary name and links it into place, so it never overwrites a file
and never leaves an archive written in part. `pack` writes an archive
kept as a folder, as Link keeps its own agents, by the same writer,
refusing a file in no thing's folder.

### Carrying Files in Place of a Thing's Own

Before writing, `export` asks each owner which files it carries in place
of the thing's own, giving it an empty folder under
`~/.lnk/data/archives` to write them in. A replacement outside that
folder, or a file outside the thing's folders, refuses the export. The
writer reads each replacement as it reads any file, never through a
link, and the folder is removed however the export ends.

### Packing a Prepared Folder

`pack` validates the manifest's layout and rejects files outside the named
state folders. Its state trees use `Writer`'s handle-based reads, but root
`archive.toml` and `README.md` are read by path. Prepare and inspect those
root files yourself, rather than treating folder packing as an untrusted
archive-reader boundary.

## Reading

`stage` reads an archive as untrusted data into a new folder under
`~/.lnk/data/archives` (`lnk.archive-staging`), removed when the
`Staged` value is dropped. It refuses, naming the entry:

- an archive whose first entry isn't `archive.toml`;
- a long name longer than its path limit, and any header but a GNU long name, such as a PAX one;
- a path that's absolute, holds `..`, or is longer than `limits.toml` allows;
- a link, hard link, device, named pipe or any entry but a plain file or folder;
- an entry twice;
- an entry outside the folders `archive.toml` names, but `README.md` at its root;
- more entries or bytes than `limits.toml` allows, or than the space free here;
- a thing whose folders aren't all in the archive.

Staged files are only the user's (600, folders 700), whatever modes the
archive says. However small the file, it never decompresses to more than
the most an archive may hold. Nothing in an archive is ever run.

## Importing

`import` stages the whole archive and selects the requested things. Each
selected owner checks its staged files before anything is asked or moved,
then the importer settles each name.

### Names and Placing

A name taken here, in any of its owner's parts, is
never overwritten, and neither is a name the layout keeps in its
owner's folders: in a terminal you're asked for another, and without one
the import is refused, naming `--select`. Each thing's folders move into
place together or not at all. When staging is on another disk, a folder
is copied beside its place under a temporary name and renamed into it.
Its owner then adopts it, and a failed adoption removes what was placed.
When a later thing fails, the things already imported stay, and the
error names them.

## Owners

`Owner` is what only a kind's module knows, answered from files alone:
`check` reads its files with their owning types, `running` says whether
export must refuse it, `carry` names the files export writes in place of
its own, or leaves out, so the archive holds nothing of this machine's,
`missing` lists what it needs here with each
setup command, and `adopt` writes the owner's records in `~/.lnk` for a
thing under the name chosen. The archive module answers `Owner` by
asking the owner's module questions. Tests answer it with a fake owner,
so every kind shares one suite.

## Expiry

`expired` selects a readable archive only when its manifest records a deletion
older than the retention limit and its filename still matches the deletion's
`<name>-<date>.tar.gz` or numbered form. Renaming it prevents expiry. A normal
export has no deletion marker, and unreadable archives are left alone.

<!-- docs-tree:start -->

## Docs

- [What It Builds On](#what-it-builds-on)
- [Selection](#selection)
- [`archive.toml`](#archivetoml)
- [Writing](#writing)
- [Reading](#reading)
- [Importing](#importing)
- [Owners](#owners)
- [Expiry](#expiry)

<!-- docs-tree:end -->
