# Catalog

> Every Module's Declarations, Read and Checked

`link-actions` reads every module's declarations into one checked
catalog, so each generator writing a surface, the command line first,
reads the same commands and shapes and never a file.

## Declarations

A module says what it is in TOML files beside its `Cargo.toml`, each
opening with its format's version, `schema = 1`. `Catalog::load` reads
the modules in the folders it's given, and every file but `module.toml`
is optional:

- **[`module.toml`](catalog/module.md):** the module's ID, which is its
  namespace, its kind and what it depends on.
- **[`actions.toml`](catalog/actions.md):** each action by its ID, with
  its runtime arguments, the specification kind it reads and its output.
- **[`types.toml`](catalog/types.md):** each named shape, specification kinds and
  outputs among them.
- **[`variables.toml`](catalog/variables.md):** each environment
  variable the crate defines or uses.
- **`schema.toml`:** a specification's versions and [the steps between them](../../../../../spec/docs/more/libraries/types/specifications.md#transformation-steps), read
  by `link-files`.

[Why the formats are as they are](catalog/decisions.md) has the reasons.

### Actions

An action is named whole as `<namespace>.<resource>.<verb>`, and its
command comes from that ID: `env.approval.add` is `lnk env approval
add`, and `env.env.create` is `lnk env create`, the resource left out
where it's the namespace's own name. Its `args` are its runtime
arguments, about one run: its positionals, its flags with no dot, and
core's shared `json` and `yes` (`shared.toml`). Its `spec` binds it to
one specification kind, whose fields are its field flags. The [Actions
File](catalog/actions.md) gives every key.

### Verbs

A verb is one of Link's fixed verbs: `create` and `delete`, `add` and
`remove`, `connect` and `disconnect`, `install` and `uninstall`, `start`
and `stop`, `set` and `get`, `list` and `show`, `check`, which reads and
tries without making anything, or `run`, which runs a command.

#### Verbs a Module Declares

A module may declare another verb under `[verbs]` with what it means.
One meaning a fixed verb, such as `new` for `create`, is refused with
the verb to say instead (`format.toml`).

#### Effects

Each verb [names its effect](catalog/actions.md#effects), `find`, `insert`,
`update`, or `delete`, which every action of that verb takes, and [HTTP derives its method](catalog/transport.md)
from it.

### Tools

An action a tool program serves to a model as an MCP call says so in a
`tool` table. It holds `name`, the name a model calls it by, and
`when`, the condition it's offered on, with MCP's hints. [Tool
Actions](catalog/tools.md) gives every key of the table and of the
module's `[tools]`.

#### Fills and Summaries

The module's `[tools]` table names what its program gives at runtime:
the values a description's `{name}` or an argument's `x-link-fill`
stands for, and the conditions. An argument marked `x-link-summary` is
one its call's title shows, in order. A boolean argument marked
`x-link-asks` makes its call ask you first unless it's given false, and
the program's sandbox learns it from the template's `asks`, as [`lnk sandbox tool add --ask-if`](../../../../../../modules/security/sandbox/docs/tooling/sandbox/reach.md#call-a-tool-that-runs-outside) sets it. A call's arguments are [held to
their declaration before its handler](../../../../build/docs/more/developing/modules/catalog/generation.md#tool-call-checks), as a flag is.

### Shapes

`types.toml` writes each shape in a subset of JSON Schema, the standard
vocabulary for describing JSON data, that maps one for one to a type.
The subset holds records closed to other fields, maps, lists, closed
choices of words, choices of records, and scalars with standard formats,
and a keyword outside it, such as `$ref` or `anyOf`, is refused by name.
The [Types File](catalog/types.md) gives the subset and Link's keywords.

### Names

A shape refers to another by its name, `stage` in its own module or
`box.id` in another, never by a file or a path. A name of another module
must be one its [module file](catalog/module.md) lists as a dependency, and that module must be
loaded beside it.

### Core's Shapes

Core's shapes, the ones every module shares, need no dependency: a
declaration names one as `core.os`, `core.user` or `core.duration`, and
every catalog loads core's declarations beside its modules. [Shared
Types](../../../../../types/docs/more/developing/modules/catalog/shared-types.md)
explains each and the crate they're generated into.

### Core's Two Folders

Core is two folders of one module, `core`: its program's, which declares
`kind = "program"`, and link-actions' own `core/` folder of shapes, which
declares `kind = "package"`. A catalog knows the shapes by that kind,
wherever the folder is, so a tree copied to another machine loads its own
copy. When none of the folders it's given is core's shapes, it uses the
ones built into link-actions, its `core/` files embedded as it's compiled,
so a program never reads a folder of the machine that built it.

## Outputs

The catalog resolves every name to the shape it means and fills in what
the declarations imply. An action that names no output answers by its
verb: `list` a list of its resource, `delete`, `remove`, `disconnect`,
`uninstall` and `stop` the resource's ID, its `x-link-resource`
property, and every other fixed verb the resource itself. An action
whose verb is the module's own, or whose resource is no shape, names its
output.

### Secrets

An action that reads something on its standard input names its shape
with `stdin`, as `output` names what it answers. A value marked
`x-link-sensitive` is a secret. `Catalog::sensitive` says whether a
shape reaches one through its fields, items, choices and names.
`prints_secret` and `reads_secret` ask it of an action's output and its
`stdin`, so a program recording a command's streams learns which hold a
secret from the declarations alone, never from how the command is spelled.

### Surfaces

An action is offered on every surface unless it names fewer, and a
`plumbing` action on the command line alone.

## Rules

A declaration is refused, naming its file and why, when it breaks a rule:
an unknown key or keyword, an unresolved or undeclared name, a verb that
isn't fixed or declared, an output that can't be implied, a specification kind
declared twice, or a built-in role naming an action its module doesn't
declare. A default its own shape refuses is refused too, so leaving its
field out never fails a run.

### Arguments

A runtime argument is refused where the command line couldn't read it:
an object, a switch that is
positional, required or read from a variable, a positional with a short
flag, `rest` on anything but a list of text, or positions with a gap. A
default is refused on a required argument, a switch or a list, since
only a text, a number or a choice has one. A
fallback variable is one a `variables.toml` defines or the module reads.

### Variables

`variables.toml` is each variable's one home. A crate defines the
variables it owns, each once with its type and description, and lists
every other one it touches under `[[uses]]`, with how and when it
touches it. The [Variables File](catalog/variables.md) gives every key.

#### Variables Catalog

`Crates` loads every crate's file under a workspace and refuses a
variable two crates define and a use of one no crate defines. The docs'
source audit holds each crate's production reads to its own file, and a
module's build gives its program its definitions as its inputs, which
[`lnk variable list`](../../../../../../core/cli/docs/config/variables.md#list-a-commands-variables) shows.

### Commands

One action's command is never the start of another's, so neither hides
the other, and no resource is named `spec`, core's word for `spec show`.
Core's words are in `format.toml`.

## Reading It

`Catalog::json_schema` renders a shape as a whole JSON Schema 2020-12
document, the version OpenAPI 3.1 uses, with every name it reaches under
`$defs` as `<module>.<name>`. A name stands as a reference there, the one
translation a generator needs. [Generation](../../../../build/docs/more/developing/modules/catalog/generation.md)
writes a module's Rust from it, and [Surfaces](../../../../surfaces/docs/more/developing/modules/catalog/surfaces.md)
writes every other surface: the OpenAPI document, TypeScript types and client, a page per action,
agents' tools, and the references.

### Targets and Roles

`Catalog::on` says what an action acts on: the resource its `resource`
argument names, or its project. `Catalog::role` lists the actions a role
the module comes with covers.

## Calls

Every surface calls an action's implementation with a `Call`: its action,
its typed runtime arguments, the specification it reads and who asks. The
implementation answers with an `Outcome`: completed with its output,
declined, refused, failed or interrupted, each keeping its meaning on
every surface. `call::answer_one` serves one call on a program's
standard input.

## Runtime

Generated code calls back into the package. `value` holds what a
generated type checks a value with, each refusal naming where the value
is, such as `stages[1].timeout`, and why, so a specification file and its flags are
refused alike. Its `Duration` reads `30s`, `10m` or `2h`.

### Hooks

A check a schema can't say is a hook: a function of the crate's own
`checks` module, named by the declaration. An action's `checks` run on
its arguments, or on its specification. A shape's `x-link-checks` run on the
shape wherever it's read, inside its generated `Validate`, so a package
that declares a specification kind and has no actions still checks it, and every
module reading it refuses alike.

### Command Lines

`cli` makes each declared runtime argument a command-line argument and
reads a command's matches back into its typed arguments, checked.
`cli::from_argv` reads a line `cli::argv` wrote back the same way, by
the same table.

#### Another Environment

An argument with a fallback variable reads it from this process's
environment. `cli::under` reads a command line as it would run with
another environment, a function from a variable's name to its value.
Each fallback then takes its value from that environment, and a flag
given still wins. A program that reads several lines in one process,
each meant to run with its own environment, reads each one this way.

#### Switches and Required Arguments

A switch reads its variable by clap's rule: empty, `0`, `false`, `no`
or `off`, in any case, is off, and anything else is on. A required
argument its variable gives needs no flag, and with neither it's still
refused.
