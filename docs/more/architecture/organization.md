# Repository Organization

Find a component by what it provides and which subject owns it. The
source tree separates Link's own capabilities, adapters for other products,
and shared libraries; [Architecture](README.md) explains
how those parts work together.

## Layout

Each component has one home based on its purpose. An adapter or contract
stays with the subject it serves, while a shared library belongs at the
top level only when the core or several subjects use it.

- `core/`: the `lnk` command in `cli/` and its shared protocol library
  in `framework/`.
- `packages/`: shared Link packages used by the core or several subjects.
- `modules/<domain>/`: Link's own capabilities, followed by their
  contracts and implementations.
- `interop/<domain>/`: adapters and packages for third-party
  products and services, grouped by the same domains.
- `docs/`: repository-wide explanations and references. A component's
  own documentation lives beside its code in its `docs/` folder.
- `tests/`: checks and fixtures spanning subjects.
- `scripts/`: installation and release automation.
- `web/`: the documentation website and shared JavaScript packages.

## Domains and Owners

The domains are agents, cloud, compute, security, storage, and tooling.
Within a domain, the next folder identifies the owner, followed by its
contracts and implementations. An owner is responsible for its code,
documentation, and declared locations. The
[naming rules](decisions.md#names) explain how Rust package
names follow this ownership.

## Kinds

The [Developer Glossary](glossary.md) distinguishes Link roles from Rust
build units. A Rust package may provide a Link module, package, adapter,
or contract. A README's generated index sorts what the folder holds by
kind, under Core, Modules, Packages, and Folders.

## Layers

A layer is a group folder with a `layer.toml` declaring its allowed
dependencies. These are `packages/`, `core/`, `tests/`, and each domain
under `modules/` or `interop/`. A dependency is permitted within its
own layer or a layer its declaration names, and Cargo rejects cycles.
The layers, from lower-level dependencies upward, are:

- `packages/`: nothing.
- `core/`: `packages`.
- `modules/tooling/`: `packages`, `core`.
- `modules/security/`: `packages`, `core`, `modules/tooling`.
- `modules/cloud/`: `packages`, `core`.
- `interop/cloud/`: `packages`, `core`, `modules/cloud`.
- `modules/storage/`: `packages`, `core`, `modules/cloud`.
- `modules/compute/`: `packages`, `core`, `modules/cloud`,
  `modules/security`, `modules/tooling`.
- `modules/agents/`: the other `modules/` layers, without adapters.
- `interop/agents/`: `packages`, `core`, `modules/security`,
  `modules/agents`.
- `tests/`: all the layers it tests.

### Enforcing Layers

The docs checks enforce the declarations for normal and build
dependencies, commands a module invokes, and questions it answers.
[Higher layers answer questions in the asking layer's types](../../../core/framework/docs/more/modules/protocol.md#questions), rather than
requiring the lower layer to depend on them.
Move code to its owner instead of adding [a dependency that crosses the
boundary](decisions.md#layers).

### Layer Words

A lower layer names none of a higher layer's concepts either. Each
`layer.toml` may declare the words its layer introduces, such as
`agent`, `team` and `harness` for `modules/agents/`, and [the repository
checks refuse them](../../../tests/repository/docs/more/developing/testing/repository-checks.md#layer-words) in every layer below it.

## Rust Workspaces

Each Rust workspace owns a Cargo manifest, lockfile, toolchain, build
profile and target directory. The workspace's default members
are `core/cli` and `core/framework`; select other Rust packages when
their programs or tests are needed. [Developing](../developing/README.md)
shows how to build a usable development installation and check changes.
