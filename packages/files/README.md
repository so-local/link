# Files

`link-files` keeps the files Local Link stores on this machine. It holds the installation folder, and it reads and writes files so no other user can swap, read, or change what is in them.

<!-- docs-tree:start -->

## Docs

### Pages

- [Files](docs/more/libraries/files/files.md): `link-files` is how Link keeps files on this machine: its folders, and reading and writing them so no other user can swap, read or change what's in them
  - [Migrations](docs/more/libraries/files/files/migrations.md): A migration is a schema step that runs its owner's Rust, for a change to a specification or saved file that no rename, removal or refusal can say
  - [Upgrades](docs/more/libraries/files/files/upgrades.md): `lnk upgrade` brings every file of state Link writes to the new release's shapes after your one yes, and it never rewrites history

### More

- [File Security](docs/more/libraries/files/files/security.md): Control reads refuse aliases into writable data and validate ownership, permissions and size through the handles used to read the document

<!-- docs-tree:end -->

## Code

- [`fs.rs`](src/fs.rs): reading a file only as its trusted writers left it
- [`layout`](src/layout/mod.rs): the typed Home and Hidden locations and their catalogue
- [`lib.rs`](src/lib.rs): root accessors, private writes and lock files
- [`nofollow.rs`](src/nofollow.rs): opening a path without following a link
