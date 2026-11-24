# Actions

> **Package**: [a library](../../docs/more/architecture/guide.md#kinds) that programs link, which runs nothing and owns no command

`link-actions` reads every module's declarations strictly, resolves and checks them, and keeps them as one catalog. Every generator that writes a command line, an API, or a reference reads that catalog.

<!-- docs-tree:start -->

## Docs

### Pages

- [Catalog](docs/more/developing/modules/catalog.md): `link-actions` reads every module's declarations into one checked catalog, so each generator writing a surface, the command line first, reads the same commands and shapes and never a file
  - [Actions File](docs/more/developing/modules/catalog/actions.md): `actions.toml` declares each thing a module does, keyed by the action's ID: its command, its runtime arguments and the specification it reads, with the shape it answers following from its resource and verb
  - [Module File](docs/more/developing/modules/catalog/module.md): `module.toml` names a module and what it needs, and its ID is the namespace every action, command and specification kind of the module begins with
  - [Types File](docs/more/developing/modules/catalog/types.md): `types.toml` holds a module's shapes, its resources, specification kinds and outputs among them, each named once and written in a subset of JSON Schema that maps to one type in Rust and TypeScript
  - [Variables File](docs/more/developing/modules/catalog/variables.md): `variables.toml` defines the environment variables a crate owns and declares each one it reads, sets, removes, or passes to a child, so every variable has one home and every use is checked against it
  - [REST Transport](docs/more/developing/modules/catalog/transport.md): Every action declares what it does to state, its effect, and how its answer arrives, and HTTP is derived from those declarations: the method, where the inputs go, the status of each error, whether an answer may be cached, and how a stream…
  - [Tool Actions](docs/more/developing/modules/catalog/tools.md): An action offered on `tool` may name its MCP call in a `tool` table

### Packages

- [Actions Build](build/README.md): `link-actions-build` writes a module's Rust from the catalog at build time
- [Actions Surfaces](surfaces/README.md): `link-actions-surfaces` writes every surface from the catalog

### More

- [Catalog Decisions](docs/more/developing/modules/catalog/decisions.md): Why modules declare themselves in TOML the way they do: what the formats settle, and the room they keep for later releases

<!-- docs-tree:end -->

## Code

- [`catalog.rs`](src/catalog.rs): loading modules into the catalog, and what it implies
- [`check.rs`](src/check.rs): the rules a declaration passes
- [`json.rs`](src/json.rs): shapes as JSON Schema
- [`read/schema.rs`](src/read/schema.rs): the subset of JSON Schema a shape is written in
- [`value.rs`](src/value.rs): what a generated type checks a value with
