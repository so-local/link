# Generation

`link-actions-build` writes a module's Rust from the catalog at build
time, so its types refuse what its declarations refuse, its command line
is its declarations', and every surface calls its handler through one
typed dispatch.

## Using It

A module's `build.rs` calls `link_actions_build::write` with its folder,
a file in `OUT_DIR` and the parts it wants, then includes that file in a
module. Cargo runs the build again when a declaration changes, and
nothing generated is checked in.

### Rebuilds

The build names only the declaration files that exist, because Cargo
counts a missing file as changed and would rerun the build, and
recompile everything built on it, on every command. So a declaration
file added to a module that already builds is read at the next change
to a declaration the build reads, or to its `build.rs`. Touch the
`build.rs` after adding one.

```rust
link_actions_build::write(
    &manifest,
    &out.join("declared.rs"),
    link_actions_build::Parts::Program,
)
```

### Parts

Each part writes what the one before it does, and more:

- `Parts::Types`, which a contract crate shares: each shape and specification
  kind.
- `Parts::Args`: each action's arguments struct, with an `argv()`
  writing the command line that gives them: the program's words, its
  positionals in order, its flags, and what takes the rest after `--`,
  and a `from_argv` reading such a line back by the same table. A
  tool's contract crate shares it, so the agent building a tool's
  command line and the tool parsing it read one declaration.
- `Parts::Handler`: the handler trait and dispatch, for a module other
  surfaces call.
- `Parts::Program`: the command line too, with each specification kind's `spec
  show`, and core's hidden `uninstall` where `module.toml` declares
  `[removal]`, which no `actions.toml` declares.

### Shapes Another Crate Owns

A program whose contract crate owns its shapes takes
`Parts::ProgramOver` with the path of the module its contract writes
them into, as `"::link_sandbox_contract::spec"`: its command line and
handler name the contract's types, so a shape other modules share is
one Rust type.

### Arguments Another Crate Owns

A contract that writes its program's runtime arguments too, with
`Parts::Args`, is named with `Parts::ProgramOverArgs` instead. The
program's argument groups and each action's arguments struct are then
the contract's types as well, along with each type made for one of
their arguments, such as a choice of words. The program writes only its
parser's tables. A line the caller builds with the contract's `argv()`
is read by the program into that same type, and the contract's check
hooks are the only ones.

### Other Modules

A module whose declarations name another's shapes, such as a contract
it implements or a specification kind another module owns, writes with
`link_actions_build::write_with` and an `Owner` for each: the other
module's folder, read into the same catalog, and the crate its shapes
are generated in. Each of that module's shapes is then that crate's
Rust type, never generated twice.

```rust
link_actions_build::write_with(
    &manifest,
    &[link_actions_build::Owner {
        directory: &contract,
        rust: "::link_store_contract",
    }],
    &out.join("declared.rs"),
    link_actions_build::Parts::Program,
)
```

### Dependencies

Types need `link-actions`, `serde` and `serde_json` in the crate. A specification
kind needs `link-spec`, `link-files` and `schemars` too, and the command
line `clap` and `link-core`.

## Types

Each shape becomes a Rust type named for it, `stage` as `Stage`, that
enforces what its shape says:

- **Records** are structs closed to other fields. A field that may be
  left out is an `Option`, a list among them, since a list left out and
  an empty one differ, or its default when it declares one, or an empty
  map. A record that needs nothing given has an empty value, and a field
  naming a record it sits inside, as an OS's own table of a section is
  the section's kind, holds it boxed.
- **Choices of words** are closed enums implementing
  `link_actions::value::Words`: every value in order (`all`), its word
  (`word`, `from_word`), and what it means (`meaning`) where
  `x-link-values` says, so code names a word only through its
  declaration.
- **Choices of records** are enums too, read by their tag or by the
  marking key each choice requires. A choice of a whole number and
  words, a port or `"*"`, is an enum read by which its value is:
  `Number` or `Word`.
- **IDs** are typed: a `link-id` of the kind `box` is `BoxId`, checked
  as a box's ID, and a `link-id` of no kind is checked as any Link ID.
- **Scalars** keep their checks. A duration reads as
  `link_actions::value::Duration`, a path is a `PathBuf`, a confined one
  refused outside the specification's folder, and every
  bound, pattern and length is checked. Other standard formats, such as
  `date-time`, describe a string and aren't checked.

### Names

No two shapes are ever one Rust type: an inline record whose name, its
parent's and its property's, is a declared shape's is refused, as are two
properties that are one Rust field.

### Validation

Each type implements `link_actions::value::Validate`, which refuses a
value naming where it is and why.

## Specification Kinds

A shape with `x-link-spec` becomes a specification `link-spec` reads. Its
versions and steps come from the module's `schema.toml`, and its JSON
Schema from the catalog, which is also where its field flags come from.
`link_spec::read_with`, `link_spec::over` and `link_spec::show` read it
from a file, its flags, or both, and every read validates it. A field
flag never takes the value of an `x-link-sensitive` field.

### Kinds a Package Owns

A package, such as one holding a policy shape several modules read, owns
no command, so it has no `spec show`. Each module binding a package's
kind shows it instead, as `lnk <module> <kind> spec show` (`lnk <module>
spec show` where the kind is named as the module), and the kind's page
names those commands.

## Command Line

Each action's runtime arguments are a struct, such as
`ApprovalAddArgs`, read from the command line and checked by its
`Validate`. `Command` has a variant for each action, `Cli` is the whole
command line as a clap parser, and `binding` names the specification kind each
command binds. An action binding a specification takes its file as `--spec`, and
a module with a specification kind has `lnk <module> spec show`. A dispatched
call carries its specification whole, so one naming a specification file in its arguments
is refused.

### Argument Groups

An argument group, which several actions take together, is one struct
flattened into each of them. Two groups can be joined, one laid over
the other, so each argument keeps whether it was given. An argument
with a declared `default` is therefore optional in its group's struct.
An accessor with the argument's name gives the default when the
argument is left out, as `flags.driver()` does. Its flag's help shows
the default, and the parser never sets it, so a join sees only the
flags that were given.

### Actions Off the Command Line

An action whose surfaces leave out `cli`, such as a tool an agent
calls, keeps its variant for dispatch and has no command: no parser
reads it, help never lists it, and it has no `argv()`.

### Tool Templates

A tool program's build also calls `write_tools` with its folder and a
file in `OUT_DIR`. That writes its tool actions as the template its
program reads with `include_str!` and fills at runtime with
`link_actions::tools::offer`, so its `tools/list` comes from its
declarations.

```rust
link_actions_build::write_tools(&manifest, &out.join("tools.json"))?;
```

A module whose declarations name another's, as a contract it implements,
calls `write_tools_with` and names that module's folder beside its own.
The template lists its calls by the name a model calls each.

### Tool Call Checks

`Parts::Program` also writes `check_call` for a module with tool actions.
It reads a call's arguments as its action's arguments struct and runs
that struct's `Validate`. A call is refused, before its handler, for
any of these:

- an argument not declared, or a required one missing;
- a value of the wrong type;
- a value breaking its pattern, bound, or choice.

So a model's call is held to its declaration the way a flag is. A
tool's program calls its generated `check_call` once the name is one
it offers, and answers a refusal as an error the model reads, naming
the argument.

```rust
crate::cli::check_call(name, &arguments)?;
```

A choice a fill gives at runtime, such as the scheduler's teammates,
isn't in the declaration, so the program checks it.

### Next's Graph

`NEXTS` is the module's Next graph: the module itself, as `lnk <id>`,
leading to its `module.toml`'s `next`, and each command a person types
leading to its action's `next` and, on a failure, its `fail`.
`PROGRAM`, the module's `link_core::help::Declared`, carries it with
the commands' bindings and the namespace, and answers the hidden `nexts
--json` from it before anything parses.

### Other Namespaces and Help

Core's program, `lnk`, reads a namespace it doesn't declare as another
module's: `Command::External` holds that word and every word after it,
unparsed, for its module's program. A program declaring `help` in
`[program.args]` renders its own help, its root without clap's help
flag and command, and each command under it keeps clap's `--help`
(`link_actions::cli::nested_help`). A group's default reads its flags
at the group, refusing them before another command of it.

### Groups

The module's own group says its package's description, and each
resource's group says its shape's description from `types.toml`. Each
group names the docs section explaining it, as an action's `docs` does:
the module's group its topic page's title (the `[documentation]` table
of `module.toml`), a resource's group its first action's section, and
the specification group and `spec show` the specification kind's generated page, at its
"Use It" section.

### Hooks

A check hook an action names is a function in the crate's `checks`
module. A specification-less action's checks its arguments. A bound action's
checks its specification, inside the specification's own `Validate`, so a file, its flags
and a dispatched call are refused alike.

### Entry

`link_core::help::Declared` runs the parser through the shared entry,
which takes the field flags out, answers retired spellings from the
module's history, and parses the rest.

## Dispatch

`<Module>Actions` has a method for each action, such as
`approval_add`, taking a `Call` of its arguments and specification and returning
an `Outcome` of its output. `dispatch` reads a call's JSON arguments and
specification into their types, checks them, calls the method and writes the
outcome back. A call of another shape, or for an action the module
doesn't declare, is refused before the handler sees it.
