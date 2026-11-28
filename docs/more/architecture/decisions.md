# Decisions

Why the repository's tree, its layers, its files' sizes and its crates'
names are what they are.

## The tree

Why each folder is where it is, and what the folders' names say of
who owns the code in them.

### Why does a folder say what a component provides?

So you find a component without reading a `Cargo.toml`, and know who
owns it. Link's own capabilities are in `modules/`, adapters for
third-party products in `interop/`, shared libraries in
`packages/` and the framework in `core/`. A component goes where its
primary responsibility puts it, never where its users are: a domain,
then the module, then its contract and implementations. Intuitive
placement wins over a balanced tree; a category exists only once
something needs it, never for symmetry. When a component does two
things, it splits into two components rather than moving whole.

### Why `modules/`, not `plugins/`?

A module is a part you install to compose what Link does, and Link is
modular: you install only the parts you use. Plugin is the familiar
word, but the harnesses Link runs call almost everything a plugin, so
module keeps Link's parts apart from theirs. Package stays the word for
code a program links. A Rust module shares the name, so a page that
could mean either says Link module or Rust module, as it does for
packages.

### Why `interop/`?

`interop/`, short for interoperability, holds the adapters: each fits a
contract to another company's product, whoever wrote it. Adapters stay
apart from `modules/` so code for another company's product never mixes
with Link's own, and so an adapter can leave for a repository of its own
without moving anything else. Link's own implementations (`file-store`,
`webhook`, Link Harness) satisfy contracts too, and stay in `modules/`.

#### Names turned down

`adapters/` led the root with a word a newcomer can't read. `plugins/`
suggests code loaded into its host, and the harnesses call almost
everything a plugin. `vendor/` and `third-party/` read as copied source,
where `cargo vendor` writes, and `external/` is vaguer. Integration is
the activity, and integration tests already hold the word. `providers/`,
`drivers/`, `bridges/` and `connectors/` each name something else in
Link, and `portable/` names what the contract is.

### Why are the agents in `agents/ready-made/`?

A newcomer who opens `agents/` first meets `ready-made/`, and in it
agents that work: `coach`, `personal`, `programmer` and `researcher`, each an
archive kept as a folder. The folder keeps them apart from the
`README.md` and the packing script beside them. Ready-made names only
these agents, never a harness, a model, a module, a preset or a default,
so the word always means an agent you import and start as it is.

### Why do `tests/` and `web/` stay at the root?

`tests/` holds the suites that cross subjects, and `web/` is the
npm/Turbo workspace holding every JavaScript app and package. Neither
belongs to one component.

### Why does `docs/` hold only what spans the repository?

A page about one module or package lives with it, in its `docs/`.
A page about the whole (credentials, the specifications overview, architecture,
developing, contributing, the code of conduct) is in `docs/`. Every
README links its folder's pages, so a reader finds a package's docs
from its folder.

### Why don't website URLs change when a folder moves?

A page's URL comes from its path under `docs/`, and a move keeps
that path. Links from outside keep working however the tree changes.

### Why is sqlite in `modules/`?

It's Link's own store, not an adapter for a product: it sits
beside `file-store` in `modules/agents/stores/`. `file-store` is the
default place.

### Why do the binding and bridge belong to the sandbox?

The bridge protocol and the binding's credentials define how a caller
crosses the sandbox boundary. They belong to that owner in
`modules/security/sandbox/bridge/`, with the binding beneath it.
Other subjects depend on those contracts instead of keeping their own
proof or ownership rules. Their [bridge reference](../../../modules/security/sandbox/bridge/docs/tooling/sandbox/more/bridges.md)
explains the shared boundary.

## Layers

Why the group folders are ordered as they are, and how a part learns
what a part above it has without naming it.

### Why is a layer a group folder, not a crate?

A crate's dependencies are its `Cargo.toml`'s, and Cargo already
refuses a cycle between crates. What Cargo can't see is a part filed
in the wrong place: an agent's names in a primitive, a box's setup in
the cloud provider. A group folder (`packages/`, `core/`, `tests/`,
each `modules/<domain>/` and `interop/<domain>/`) is what can be
held back, tested or understood alone, so it is the unit the layers
order. Inside a group, the crates' manifests decide.

### Why does a `layer.toml` list every layer, and no exceptions?

Each layer lists every group folder it is built on, none implied
through another, so the file alone says what the layer may use. It
states the target, never today's crossings: a list of exceptions would
make a crossing permanent. A part that would cross moves to where it
belongs, and a `depends_on` is never added for it.

### Why are agents the top layer?

Agents are an application of Link: they run on boxes and in sandboxes,
as any program does. Nothing below knows of agents: no agent contracts
in the cloud provider, no agent stores under buckets.

### Why does the cloud provider know nothing of Link's?

It is the shared interface to AWS and Google Cloud: machines, images,
buckets and the calls each cloud answers. Boxes, buckets and agents
are built on it, so it names none of them. An agent's commands are its
own contract's, asked through the provider's generic call.

### How does a module learn what a module above it has?

It asks a question (`link_core::answers`), and each installed module
that answers it does. The asker's layer defines the question and both
its types; the one above names in its manifest the command that
answers. So the asker names no module above it, the types are one
source both sides share, and an answer is always current: whether an
agent runs, which accounts answered. A file the module above writes
for the one below would go stale, and couldn't carry an action, such as
removing a secret's copy on a box.

## Files

How a file that has grown too large is moved and split, and why it
may only shrink while it waits.

### Why do moves and splits happen apart?

Moving is mechanical and checked by the build; splitting changes code
and can change behavior. Every move comes first, so each split happens once, at the component's
final home.

### How does a large file split?

Where a responsibility ends: its items move unchanged into child
modules, visible to the files that call them, and a library's public
module keeps its paths, its children private and re-exported. Each
test moves with what it tests; an integration test becomes
`tests/<name>/main.rs` with modules beside it, keeping its target
name. A trait impl too large for one file keeps its methods in the
parent, each calling a same-named method in a child that holds the
body. Any list or test that names a file by path follows the code.

### Why may a file over its limit only shrink?

The no-growth rule keeps a large file from getting larger while it
waits to be split, and `large-files.toml` names each one with its
lines, lowered as it shrinks. No Rust file is over 1,000 lines.

## Names

How a crate is named, so its name says what it is and who owns it.

### Why does a module declare its id?

A module's id is declared once, as `id` under `[module]` in its
`module.toml`, and its one program is `lnk-<id>`. The component
metadata, the registry and the release read that id, never the crate's
name, so a crate is named for its subject (`link-agent`) like any other.

### How is every other crate named?

Every crate starts with `link-`, and its subject is the module, package
or service that owns it, so the shared framework is `link-core`. Core's
program is `lnk`, a module's is `lnk-<id>`, and any other program is
`link-<name>`, so only what `lnk` runs starts with `lnk-`. A test
fixture that `lnk` runs as a module is one, as `lnk-connection-fixture`
is.

### How does a name show where a crate sits?

A crate inside another's folder takes that crate's name as a prefix:
whatever sits in `sandbox/` is `link-sandbox-<part>`, such as
`link-sandbox-bridge`. A folder's name is the crate's name without
`link-` and without its parent's prefix, and a group folder is plural
while a crate in it is singular, so `connections/contract` holds
`link-connection-contract`. A protocol with a name of its own keeps it,
as `link-agent-state-protocol` does.

### What does a suffix say?

The suffix says the crate's role, from a closed list: none for the
module or package itself, `-contract` for an interface holding no
implementation, `-derive` for a procedural macro, `-build` for a build
script's helper, `-fixture` for a test stand-in and `-check` for a test
suite. One name never means two things, so the cloud providers' contract
is `link-cloud-contract`.

### How are modules of one kind named?

Modules of one kind are named alike. One of Link's own named by a
generic word ends in its kind, such as `link-file-tool`,
`link-sqlite-store` or `link-webhook-connection`, and one named after a
third-party product is that product's name, such as `link-hermes` or
`link-telegram`.

### Which words does a name avoid?

A name avoids the overloaded words host, key, home and local for here,
and any phase or status word, such as next, new, v2, private or public.
A name says what a crate is, which outlasts where it is in its life.

## Saved files

The runtime layout separates what a person manages from what Link manages.

### Why Home and Hidden?

Home is for work, editable configuration and records a person inspects.
Visibility follows that purpose: a database index may belong beside the
files it describes. Native harness homes belong in Hidden because their
tools create dotfiles. Installation identity, grants and credentials stay
outside those harness homes.

### Why categories inside Hidden?

Hidden groups files by lifetime and responsibility: cached downloads can
be rebuilt, runtime endpoints disappear with their processes, and durable
state survives restarts. The [file layout](../reference/locations.md#inside-lnk)
defines the categories and their contents.

### Why one catalogue in `link-files`?

Every layer needs paths, including code that does not use the module
framework. The existing files package owns their typed identifiers and
physical layout. Callers select a location and add their own names;
remote bootstrap recipes receive the same catalogue's paths.

### Why are temporary siblings visible?

An atomic replacement must stage on the destination's filesystem.
A visible temporary name preserves that guarantee for Home and explicit
output destinations while keeping hidden bookkeeping inside Hidden.
