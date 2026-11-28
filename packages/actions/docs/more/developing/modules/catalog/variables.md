# Variables File

`variables.toml` defines the environment variables a crate owns and
declares each one it reads, sets, removes, or passes to a child, so every
variable has one home and every use is checked against it.

```toml
# variables.toml
schema = 1

[defines.LNK_MEMORY_MODEL]
type = "string"
description = "The embeddings server the memory tool reaches, when --embed-url is absent"

[[uses]]
name = "HOME"
access = ["read"]
context = "runtime"
```

## Definitions

`[defines.<NAME>]` is the variable's one definition, with its type, its
description and, where it has one, its default. One crate defines a
variable, and every other crate using it declares the use without
repeating the definition. Core defines the outside variables Link reads
or hands on, such as `HOME`, `PATH` and the proxies, once.

### Secrets

A variable whose value is a secret names it instead of holding a
default, as `carries = { secret = "telegram.bot_token" }`, the
credential by its owner. Telegram's and Discord's connections define
their tokens this way.

### Families

A family of variables, one for each of something, is defined once by its
start and a `*`, with `suffix` saying what the `*` names. Every member
has the family's definition, and a crate using the family declares the
pattern, or the one member it reads.

### Names

A variable's name is letters, digits and `_`, starting with a letter,
in capitals. An outside program's variable may instead be all lowercase,
as the system's proxy variables, such as `https_proxy`, are. Link's own,
`LNK_`, are always capitals.

## Uses

Each `[[uses]]` entry names a variable, how the crate touches it, any
of `read`, `set`, `unset` and `forward`, and its context, `runtime`,
`build` or `test`. A use of a variable no crate defines is refused [when
the catalog loads every crate's file](../catalog.md#variables-catalog).

### Namespaces and Readers

A name the crate computes rather than writes out is a pattern with a `*`
under `[namespaces]`, saying what chooses the rest. A function whose
callers name the variable it reads is under `[readers]`, by its path in
the crate, saying which variables those are. Neither lets the crate read
a name it writes out without declaring it.

```toml
# variables.toml
schema = 1

[namespaces]
"LNK_*" = "A program's name in capitals, read as that program's stand-in"
```

## Flags and Specifications

A runtime argument names its fallback variable with `x-link-cli`'s
`env`, as `embed-url = { type = "string", x-link-cli = { env =
"LNK_MEMORY_MODEL" } }`, and the variable must be defined in a `variables.toml`. [A specification
field never reads a variable](../../../../../../../docs/more/developing/modules/specs.md#variables), so a printed specification reproduces its run
wherever it runs.

## Reports

Each command's help and [`lnk variable list`](../../../../../../../core/cli/docs/config/variables.md#list-a-commands-variables)
come from every crate's file, with each variable's owner, its uses and
the commands that read it.

### The Source Audit

The docs' source audit reads each crate's production code and holds
every use of a variable to the crate's own file, with its access and
its time:

- `read` at `runtime`: `std::env::var`, `var_os` and `Secret::from_env`,
  and a function the crate lists under `[readers]`
- `set` at `runtime`: `std::env::set_var`, and a child's
  `Command::env` and `envs`
- `unset` at `runtime`: `std::env::remove_var`, and a child's
  `Command::env_remove`
- `read` at `build`: `env!` and `option_env!`, as the crate is compiled

A crate that defines the variable may touch it any way; any other
declares that use, by name or family, with that access and context.
A name the code computes needs its pattern under `[namespaces]`, and a
name a function takes from its caller, or every variable at once
(`std::env::vars_os()`), needs that function under `[readers]`. A
child's `env_clear` names no variable, since it only narrows what the
child gets. The variables Cargo sets for every compile, such as
`OUT_DIR` and `CARGO_PKG_VERSION`, are Cargo's, and need no
declaration to be read as the crate is built.
