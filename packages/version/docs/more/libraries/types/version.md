# Version

`link-version` is a version of `lnk`, as semantic versioning writes and
orders them: the one type every place that reads, compares or carries
one uses. It also holds [the version of each protocol between programs](#protocol-versions). Commands that compare release
versions share this parser rather than interpreting a version string themselves.

## What a version is

Three numbers (`0.0.1`), then perhaps a prerelease
(`0.0.1-staging.202610131900`), read by semantic versioning's rules: at
most one `v` before it, no build metadata (`+...`), and each number
digits alone, no leading zero, within a `u64`. Why a text isn't one is
`NotAVersion`.

## How versions order

By their numbers, then a prerelease before the release of the same
numbers, and prereleases part by part, numbers by value before words.
So a staging build comes before the release it precedes, and a later staging
build after an earlier one.

### The release a build is of

`Version::release` is a version's numbers alone, the release it's a
build of: a staging build of 0.0.2 is of release 0.0.2, though it orders
before it. A command history holds its entries to that release, so a
staging build of the release that retired a command runs, as the
release will.

## Protocol versions

A protocol is what two programs say to each other over a socket or a
pipe, such as a bridge's challenge, proof and answer. Its owner declares
it in a TOML file beside its code, read once into a
`link_version::Protocol` (`Protocol::shipped`), with four keys and a
fifth once `oldest` is raised:

- `name`, what people call it (`bridge`).
- `current`, the version this release speaks.
- `oldest`, the oldest version it accepts of a peer.
- `upgrade`, what a peer too old runs to catch up, as [`lnk upgrade`](../../../../../../docs/more/modules/guide.md#upgrade).
- `bridge`, the newest release that still accepts the version below
  `oldest`, [named only once `oldest` is past 1](#raising-the-oldest).

`current` is the version this release speaks, and `oldest` the oldest it
still accepts of a peer, from 1 up to `current`. Programs of one release
speak one version of each protocol.

### Module questions

Each question one module asks the modules built on it has a protocol of
its own, declared beside the question by the module that asks it
(`measure traffic answers` for `measure.traffic`), carried in [the
question's and the answer's envelopes](../../../../../../core/framework/docs/more/modules/protocol.md#questions).

### Descriptions and history

Two protocols are between `lnk` and the programs it runs. [The
description protocol](../../../../../../core/framework/docs/more/modules/protocol.md#the-description-protocol) carries what a module answers `--link-describe`
with and the registry `lnk --link-registry` prints,
and [the history protocol](../../../../../../core/cli/docs/more/architecture/files.md#the-history-querys-shape) what a downloaded core answers `lnk upgrade`'s
`--link-history` with.
#### The answer's version

The asker states its version in a variable, and the answerer refuses
one older than its `oldest` with `accept`, which returns the version
it accepted (`Accepted`). It answers in the asker's version, its own
where the asker stated none or a newer one (`Protocol::answering`,
which takes only an accepted version), so the asker reads the answer with
`Protocol::read`: an answer in a version it doesn't read, older or
newer, is refused by name before any field of it is read.

### Connecting

Both sides state their `current` when they connect, and each checks the
other's with `Protocol::accept`: a peer older than `oldest` is refused
by name, as `the bridge speaks the bridge protocol 1, below 2, the
oldest this release accepts: upgrade it with lnk upgrade`. A newer peer
is the newer side's to judge, since it knows both versions.

### Reading a one-way message

A stream or a command whose sender hears no version back, such as an
agent's move stream, gives the sender nothing to judge by. Its reader
checks the version the message states with `Protocol::read`, which
refuses an older one as `accept` does, and a newer one naming this
release's upgrade: `the stream speaks the move protocol 5, newer than 4,
the newest this release reads: upgrade this one with lnk upgrade`.

### Changing a protocol

A change to what the protocol carries advances `current`. A release that
can no longer read the older shape raises `oldest` with it, and an
answerer keeps a writer for each version it still accepts.

### Raising the oldest

A peer below `oldest` is told to run `upgrade`, which works for a
protocol `lnk upgrade` doesn't speak itself. For the description,
history and upgrade protocols, though, an older core asking the
newest release would be refused again. Their declarations
(`Protocol::shipped_upgrade`) so name a `bridge` release once `oldest`
is raised, and the refusal names it: `upgrade it with lnk upgrade
0.20.0 first, the newest release accepting it, then with lnk upgrade`.

### Protocol and release versions

A protocol's version is a number of its own, apart from `lnk`'s release
version, a saved file's schema and a journal's version. Several
releases may speak one protocol version, and a protocol changes only
when what it carries does.

