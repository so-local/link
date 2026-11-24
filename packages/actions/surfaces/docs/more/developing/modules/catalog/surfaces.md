# Surfaces

`link-actions-surfaces` writes every surface Link offers beside its
command line from the catalog of every module's declarations, so the
API, its typed client, agents' tools, and the references can never disagree about
a command or a shape.

## Generators

A module declares its commands in `actions.toml`, its shapes in
`types.toml` and its environment variables in `variables.toml`.
`link-actions` reads every module's files into one checked catalog, and
each generator here reads that catalog and nothing else. A generator
adds no behavior, so what a surface offers is what the declarations say.

### Where an Action Is Offered

An action's `surfaces` field says where it is offered. A plumbing action
is offered on the command line alone, so no generator here writes it.
Core's `--json` flag shapes only what the command line prints, so the
API, its client, and agents' tools leave it out. Core's `--yes` answers for a
person what a command would ask, so agents' tools leave it out, and a
model never says yes for the person. `surfaces.toml` lists both kinds.

### One Module a Surface

Each generator is one module of this package.

- **`openapi`:** the OpenAPI document.
- **`typescript`:** TypeScript types and a typed client.
- **`rust`:** Rust types and a request per action.
- **`pages`:** a web page per action.
- **`tools`:** agents' tool schemas.
- **`reference`:** the command reference, specification references, and variable
  index.
- **`completion`:** `lnk`'s shell completion for bash and zsh.

## OpenAPI Document

OpenAPI is the standard description of an HTTP API, read by client
generators, documentation sites and test tools. Version 3.1 describes
its shapes in JSON Schema 2020-12, the dialect the catalog renders, so
the one translation the document needs is where its references point.

### Paths and Components

Each action is a request to `/_link/<namespace>/<resource>/<verb>`, and
its operation ID is its action ID. Its method and where each input goes
are its effect's, as the [REST Transport](../../../../../../docs/more/developing/modules/catalog/transport.md)
derives them: a find's or a delete's arguments are query parameters, and
an insert's or an update's other inputs and specification are its body. Its
output is the `200` answer, or a stream's events as `text/event-stream`
with `x-link-event` naming their tag, and a find's answer carries the
`Cache-Control` its `keeps` gives. `4XX` and `5XX` are core's error.

### Components and Extensions

Every shape is a component named `<module>.<name>`. An operation's
`x-link-command`, `x-link-role`, and `x-link-effect` give its command,
where help lists it, and its effect, and each parameter's `x-link-cli`
says how the command line takes it.

## TypeScript

`types.ts` declares a type for every shape, named in PascalCase from its
qualified name, so `env.stage` is `EnvStage`. It also declares each
action's `Args` and `Output`. Two names that would read alike are
refused, never renamed.

### Typed Client

`client.ts` exports `client(transport)`, whose methods follow the action
IDs, as `client(transport).env.approval.add(args)`. Each method builds
its call, its method, query, body, and stream tag, and passes it to the
transport its caller gives it, holding no behavior of its own. A
streaming action's method yields its typed events. `statuses` gives each
named error's status.

## Rust

`types.rs` declares a type for every shape an API action reaches, named
as TypeScript names it, and `client.rs` each action's `Args`, `Output`,
and a function, named by its ID's words with `_`, answering its request:
its method, query, body, and stream tag. A record or word list in place
becomes a type named by where it stands, and two names that would read
alike are refused. The crate including them supplies the request type
and sends nothing itself.

### Action Pages

The page per action is a route at
`<namespace>/<resource>/<verb>.tsx`. Each renders one component, which
the caller names, with its action ID, so the page holds nothing else. The
component finds everything about the action in its operation in the
OpenAPI document.

## Agents' Tools

A tool is an action a model may call. Its name is the action ID with `_`
between the words, as `env_approval_add`, since tool names allow no dot.
Its description is the action's, and its input is one JSON Schema object
holding the runtime arguments and, under `spec`, the specification. Every shape
the input reaches is under the input's `$defs`, so each tool stands
alone. No `x-link-*` keyword is left in an input, so a model's provider
never sees one. `tools.json` holds every such tool.

### Refused Tools

A name longer than `surfaces.toml`'s limit of 64 characters is refused,
and so is an action with a runtime argument named `spec`, since its tool
holds the specification under that name.

### Tool Programs

A module whose program serves its own MCP calls, an agent's tool, gives
each a `tool` table, whose name a model calls in place of the joined ID.
Only its program offers those calls, so `tools.json` leaves them out:
`tools::template` renders them, each input as a tool's, and the program
gives them, so one source names each call. Each call carries the
arguments its title shows, the choices and text its program fills, and
the condition it's offered on, for `link_actions::tools::offer` to fill
at runtime and give as `tools/list` with the title's arguments in
`_meta` `link.local/summary`.

## References

Each module's command reference is at
`docs/more/reference/modules/<domain>/<module>.md` in its own folder,
its domain the folder after `modules` or `adapters`, as `tooling` for
`modules/tooling/bundle`. It lists every command the module offers, in
order of its resource, with its usage, its runtime arguments, the specification
it reads, and what it answers. An argument kept out of help (`hidden`)
is kept out of its usage and arguments too.

### Hubs

The workspace's hub, `docs/more/reference/modules.md`, links a page for
each domain, `docs/more/reference/modules/<domain>.md`, which links
that domain's modules. The hub's pages stay few as modules grow, within
the docs' limit on pages under one page. A domain with fewer modules than
the docs allow a group, three, has no hub of its own: the workspace's hub
links its modules directly.

### Specification Pages

Each specification kind's page is at `docs/config/specs/<kind>.md`, beside its
JSON Schema. It opens with the commands a person gives the specification to:
those on the command line, never plumbing, of the page's own workspace.
The page lists every field by its key path, with its field
flag where it has one. Each flag is the one the command line's parser,
`link-spec`, derives from the same JSON Schema, so the page and the
command line can't disagree. A table has no flag of its own, but each
value inside it does, as `--server.port`, and a map's values do by their
key, as `--variables.<name>`.

### Variable Index

The workspace's variable index lists every variable a module defines,
with its type, its default, and its meaning. A runtime flag that reads
one when it is absent says so in its command's reference.

### One Page Each

Each reference is one page, however long: a module's commands, a specification
kind's fields, and the variable index. A reader looks an entry up by its
heading, on the website or in GitHub's outline, so a page is never split
to fit a length. The docs' limits on lines and headings hold every page
a person writes, and `tests/docs/limits.toml` exempts these. A module
whose commands act on several resources has a heading for each resource
and its commands beneath, so the page's outline is the module's
resources.

## Shell Completion

The completion generator writes `lnk`'s bash and zsh scripts into the
workspace's `scripts/completion/`, for the workspace holding core. Each
script holds a table of the words a person may type after each command
so far, from every action offered on the command line but plumbing, and
a function the shell runs on Tab that walks it. A command's runtime
flags come from its arguments, an enumerated one completing its values,
and its field flags from its bound specification through `link_spec::fields`,
the list its help prints. Where to put each is on
[Shell Completion](../../../../../../../../core/cli/docs/more/reference/completion.md).

## Freshness

A generated file, such as the OpenAPI document or a page of the docs,
is written by its generator and checked in. Every generated file carries the line
`surfaces.toml` names as the marker. `Files::problems` reports a file that
is absent, stale, or edited by hand. It also reports a file a person
wrote where a generated one goes, and a generated file in a generator's
folder that nothing writes any more. `Files::write` writes the absent
and stale files, removes the orphans, and refuses to overwrite a file a
person wrote.

### Generated Files

A file is generated only if it carries the marker on its first line, or as its top-level `x-link-generator` in JSON, so a person's
page quoting the marker is never removed. Each module's command and specification
folders are the generators' own, so a page whose kind was renamed or
whose commands are gone is found and removed.

### Modules Read

`modules::discover` finds the modules a workspace's checks generate
from: each member of its Cargo workspace whose folder holds a
`module.toml`, except the members a held list names. The public docs
check holds the references fresh, and `lnk-docs-inventories --write`
writes them.
