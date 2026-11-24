# Architecture

> Architecture From Parts to Programs

Local Link programs are assembled from a small core and independently
installed parts. This page follows a command through those parts. The
[Developer Glossary](glossary.md) defines their names, and
[Repository Organization](organization.md) explains where their code belongs.

## Programs and Their Parts

A command such as [`lnk agent start`](../../../modules/agents/agent/docs/agents/README.md#create-an-agent) enters through Link core and reaches
the executable providing the requested behavior. The Agent program uses
the Agent module, a harness implementation and the other modules or
adapters the user selects. Each part can be installed independently;
composition does not require one large executable containing them all.

## Core, CLI and Framework

[Link core](../../../core/README.md) comprises the command-line
executable and its shared library. The [CLI](../../../core/cli/README.md),
`lnk`, routes commands and manages installed software. The
[framework](../../../core/framework/README.md), `link-core`, supplies
descriptions, discovery, installation and signed-release handling used
by the executables. Domain behavior belongs in the selected parts.

## Modules and Packages

A Link module supplies an installable capability through its own
executable. Other modules invoke its documented commands rather than
linking its implementation. Link packages supply library code used at
build time. [Shared interface types and clients belong in contracts](../modules/contracts.md), so
callers depend on the interface rather than a particular implementation.

## Contracts and Adapters

A contract describes the operations and behavior a caller expects.
An adapter implements those operations for another product. The
OpenClaw adapter and Hermes adapter each implement the harness contract.
The [harness interface](../../../modules/agents/harnesses/portability/contract/docs/agents/harnesses/more/protocol/README.md)
specifies their behavior. Call a part an adapter when explaining its role.
Installation as a module is a separate classification.

## Executables and Processes

The CLI invokes the executable that owns a command. On Unix the CLI replaces
itself with that executable and keeps its process ID. The selected
program can then start further processes. A Link program's composition
can therefore span multiple executable files and running processes.
The [discovery protocol](../../../core/framework/docs/more/modules/protocol.md)
explains how the parts identify themselves and communicate.

## Trust and Removal

The sandbox controls the folders a running agent can change and the
services it can reach. Installation is a separate responsibility. Local Link
organizes the software it installs, including third-party harnesses,
and provides removal with an explicit choice about saved data.
The footprint and its protected boundaries are in
[Where Link Keeps Things](../reference/locations.md). The core's
[Files and Variables](../../../core/cli/docs/more/architecture/files.md)
reference lists the files and environment variables Local Link's programs read.

## Words and Implementation

Each product has one name, and [Product Names](names.md) distinguishes the
products used in these explanations. Read why Local Link is
[Built in Rust](rust.md) and where TypeScript belongs.

## Implementation Details

Dependency contracts and the runtime mechanisms behind these relationships
are in the [Architecture Guide](guide.md). The guarantees and their limits
are in [Security](security.md), and the rationale is in
[Decisions](decisions.md). Build and contribution procedures belong in
[Developing](../developing/README.md).

<!-- docs-tree:start -->

## Docs

### Pages

- [Architecture Guide](guide.md): `lnk` is a small core that runs a module for each part of Local Link, and every agent runs in Link's sandbox, which lets it touch only what you allow
- [Developer Glossary](glossary.md): Names for the programs you run, the parts you install, and the Rust code developers build
- [Built in Rust](rust.md): Every program in Local Link is written in Rust: `lnk` and its modules, Link Harness, Link's tools and the harness adapters
- [Product Names](names.md): Every name in title case that Local Link's docs, websites and messages use for the company and what it makes, with what each one means
- [Repository Organization](organization.md): Find a component by what it provides and which subject owns it

### More

- [Decisions](decisions.md): Why the repository's tree, its layers, its files' sizes and its crates' names are what they are
- [Security](security.md): How Link protects the people who use it, what it doesn't protect against, and where each part's details are

<!-- docs-tree:end -->
