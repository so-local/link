# Component Build

`link-component-build` is what a package's build script calls to embed
its component description, read from Cargo and its declarations, and
the pages it owns. Discovery never runs an installer or resolves Cargo:
[everything a program says of itself](../../../../../../docs/more/libraries/types/component-metadata.md) is already in its binary.

## `embed()`

Called by a module's build script. It resolves the package's
`module.toml` and Cargo manifest into `Resolved` and writes it as
`component.json` in `OUT_DIR`. It writes the description's history
beside it as `history.json`, which every command's parser reads for its
retired spellings without reading the description and its pages. It
also writes `external-pins.rs`: a
constant of each pin `module.toml` names, read from the file it points
at in the package.

### What it refuses

A module owns exactly one program, `lnk-<id>`, named for the `id`
its `module.toml` declares under `[module]`. An option schema, a pin or a page
outside the package is refused, as is a pin alias that isn't letters,
digits and dashes, as is a page that's a symbolic link ("documentation must
stay in its owner") or a documentation topic named twice.

### Its pages

The pages are the package's `docs/` Markdown, except the root `navigation.md` and `website.md` metadata and
each translation (`<page>.<language>.md`, beside its English), and
those of the folders under it that aren't another module's.

## `core()`

The core's registry, generated from the resolved source declarations:
each workspace member with a `module.toml`, resolved and validated as
one registry (`registry.json`), refusing a module identity named twice
and a member pattern not expanded. Its pages, with `core/framework`'s, go
in `documentation.json`.

### Product components

`LNK_COMPONENTS` is an explicit build input: local component directories
separated as `PATH` is, relative to the workspace root or absolute.
Each is resolved from its owning Cargo workspace, must have this core's
version, and cannot replace a registry identity. With no input, only the
core's workspace is read. Build and release packaging use the same
input; services cannot add declarations or change the release signer.

## `manual()`

Shared guides and documentation-only subjects belong to the optional
manual: each root `scripts/documentation-index.json` names, but `web/`,
`core/` and those inside a module, gives its pages to `manual.json`.
