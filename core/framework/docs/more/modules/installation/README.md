# Module Installation

> Installs That Check Before They Trust

`link-core` is the package that installs, upgrades and removes modules
for `lnk`, and checks each program before it runs. This page follows
how it finds a release and what it checks, while the
[Modules Guide](../../../../../../docs/more/modules/guide.md) tells the
same story for the person typing `lnk`.

## Where a Program Comes From

A released `lnk` installs a module from the release it reads: the one
`LNK_RELEASES_URL` names, else the bundle `lnk` runs from, else this
machine's copy of the release `lnk` is, as a box made from your own
build keeps, else Link's. A module counts as installed only when its
program is beside `lnk`, as `link_core::installed_module` decides in the
[Module Protocol](../protocol.md#lnk-mod). A source-built `lnk` builds
from its recorded checkout instead, which
[Install `lnk`](../../../../../../docs/more/developing/setup/install.md#add-modules-with-cargo)
explains.

## What a Module Needs

Each module's `module.toml` declares what it needs and what it goes
with, and the core's registry derives the installation order from those
declarations alone. [Requirements and recommendations](../protocol.md#requirements-and-recommendations)
says how a required operation carries its options and platforms.

## Releases Link Signs

Every program Link releases is checked against Link's signer before it
is put in place, and no older release comes back unless you ask for it.
[Signed Releases](releases.md) covers the signer, each program that
checks it, downgrades, download limits, and the release workflows.

## Releases Others Sign

A module from outside Link is checked against its maker's key instead,
which `lnk` pins the first time. [Outside Module Releases](outside.md)
follows where that release comes from and what `lnk` refuses in it.

## Why Installation Works This Way

The choices behind separate programs, declared needs, uninstall parts,
and the installer's version floor are in [Decisions](decisions.md).

<!-- docs-tree:start -->

## Docs

### Pages

- [Outside Module Releases](outside.md): A module from outside Link comes from its maker's own signed release, never from Link's
- [Signed Releases](releases.md): How a release is signed, who checks it, and what keeps an older one from coming back

### More

- [Decisions](decisions.md): Why `link-core` installs, declares and removes modules the way it does

<!-- docs-tree:end -->
