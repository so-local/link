# Settings Security

Folder relocation treats harness files as writable data and keeps every
rewrite inside the directories it opened.

## First Need

Inspection creates no control roots. Before a run starts, execution
preparation creates and protects the Safes and installed-code root,
and the runtime socket root. The sandbox then derives its masks from
those existing paths, including sockets a later command might create. A missing
remembered Link folder is refused before creation, so a moved folder's
old path cannot become a second control tree.

## Held Directories

Direct rewrites, folder moves and arrived homes use the same
component-by-component checked walk. User-controlled links in the root's
path are refused; child links are skipped. Once a parent is open, reads,
temporary files, renames and cleanup use that handle. Replacing its old
path with a link to a Safe cannot redirect these operations.

## File Snapshots

Each source is opened without following its final link, then checked as
a regular file and read through that handle with a size bound. Changed
text is written to a new file in the same held parent; its original
mode is applied through the new file's handle before the atomic rename.
A concurrent replacement of the destination is replaced as a directory
entry, never followed. Relocation does not lock harness writers or
promise a transaction across the whole tree.

## Rewrite Inputs

The typed `relocation.toml` bounds text reads and declares database
headers and companion suffixes. Oversized or non-UTF-8 files, SQLite
databases and their journals remain untouched. Database detection reads
bounded snapshots through the same held parent. Sandboxes separately
protect [control documents and installed programs](../../../../../../files/docs/more/libraries/files/files/security.md).
