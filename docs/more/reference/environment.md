# Environment Variables

Find the environment variables declared by Link's commands and libraries,
including internal and test overrides. Each link opens the declaration that owns
its meaning, default and supported values. List the variables declared by the command you are configuring.

```sh
lnk variable list agent start
```

## Read a declaration

Each crate declares its variables in a `variables.toml` beside its
`Cargo.toml`: each variable it owns, defined once with its description,
and each one it uses. A module's definitions are its program's inputs,
and a flag may name one as its fallback. The index links each
definition, including dynamic namespaces. For command variable names and descriptions, use [`lnk variable list`](../../../core/cli/docs/config/variables.md#list-a-commands-variables), or
`lnk variable list agent start` to narrow the list. Current values are never read.

## Declared variables

Every definition, by the crate defining it, with its type, its default
and its meaning, is in the [Variables](variables.md) reference,
generated from every crate's `variables.toml`. The source audit
requires each crate to declare every variable its production Rust code
reads, sets or removes, Link's `LNK_*` and the system's alike, such as
`PATH`: a name it defines or declares a use of, a namespace it declares,
or a reader function whose caller names it. Variables consumed only by
external programs remain outside that audit's scope.

## Dynamic namespaces

These patterns describe variable families read by the owning library.
A pattern does not authorize a new literal variable name. Caller-selected
reader functions are declared beside them, under `[readers]` in the same
`variables.toml`. The [Variables](variables.md) reference lists each
namespace beside the crate's definitions.
