# Catalog Decisions

Why modules declare themselves in TOML the way they do: what the
formats settle, and the room they keep for later releases.

## The Formats

What every declaration file shares, whichever module keeps it.

### Why is TOML the source of every shape?

A command, a flag, a specification field, an output and a variable each have one
declaration, and every surface is generated from the catalog that reads
them. Rust that defined a shape too would be a second source, free to
disagree with the command line, the API or the docs. So no Rust type,
derive or attribute defines one, and a check TOML can't express is a
hook the declaration names, run by the generated code.

### Why a subset of JSON Schema?

JSON Schema is the standard vocabulary for JSON data, and OpenAPI 3.1
uses it as it is, so the API document needs one translation, a name to
a reference. The subset holds only what maps one for one to a Rust
type and a TypeScript type, so a shape never means one thing to one
surface and another to the next. A keyword outside it is refused by
name rather than half supported.

### Why does every file name its version?

Each file opens with `schema = 1`, so a later release that changes a
format reads an older file through its migration channel, never by
guessing its shape from the keys it holds.

### Why names, never paths?

A declaration refers to a shape by its name, `env` or `box.id`, and
authors never write `$ref`. Where declarations are kept can then change
without a reference changing, and a name of another module is checked
against the dependencies its `module.toml` lists, so a module can't
reach a shape it doesn't depend on.

### Why does a specification file still name another by its path?

A specification file is yours, kept where you work, and names another specification by
its path, as an environment's stage names its sandbox specification. A path is
what a person can open and edit, while declarations are the program's
and need names that survive a move.

## Actions

How an action is named, and what it leaves unsaid.

### Why is an action keyed by its ID?

An action is `<namespace>.<resource>.<verb>`, and its command comes
from that ID, so the command line can't drift from the declaration. The
same words read as an access-control action, so a role or grant can
name what a command does with no rename later.

### Why are the verbs fixed?

A person who knows what `create` or `remove` does in one module knows it
in every module. A module may declare a verb of its own with its
meaning, and one meaning a fixed verb is refused with the verb to say
instead, so two words never come to mean one thing.

### Why is what's implied left out?

An action's resource comes from its ID and its output from its verb,
declared only where they differ. A repeated value is one that can
disagree, and most actions then declare little beyond their arguments
and specification.

### Why is every output declared?

Every action's `--json` answer has a type, so the API, the app, model
tools and the docs say what a command answers from the same shape a
program reads, and a change to an answer shows in the build.

### Why do suggestions name actions?

`next` and `fail` name action IDs, never text with placeholders. A
suggestion then can't name a command that doesn't exist, and the IDs
together become Next's graph, which every surface reads the same way.

### Why does core give `uninstall` and `spec show`?

Every module has them and they work the same in each, so a module never
declares them and none can differ. Core dispatches every namespace to
its module's program, which never sees its namespace, and declares its
own verbs and flags as any module does.

## Shapes

How `types.toml` tells one shape's kinds apart.

### Why is a choice told apart by a tag?

A shape of one of several kinds says its kind by a tag naming it, so a
reader never infers it from which other values are there, and adding a
kind can't make an old file mean something new. The one exception is a
shape where a tag would be clearly worse for the person writing it, as
an environment's stage is: each choice is then named by the required
key only it has, declared in `x-link-markers`, so the rule stays
checked rather than inferred.

### Why is a public key a format of its own?

A switch's secrets are sealed to the receiving end's public key, which
crosses the hosting commands as base64 text. `seal-to` reads it into
`link_secretive::SealTo`, as `duration` reads into its own type, so a
key that isn't one is refused where it's read and no handler parses
text.

### Why may a hidden argument's payload name a Rust type?

What one Link program hands another, as sandbox's `--with-fd` payload,
can hold parts the format can't say yet, such as a secret whose rule's
fields sit at its own level. Teaching the generator that form, or moving
those types below the crate that declares the payload, would cost more
than a payload only one release's programs pass is worth. Such a part
names its Rust type instead (`x-link-rust`), and each declaration that
does is on the shapes check's list, which only shrinks.

### Why do some shapes stay in Rust?

A few types still read a generated value through JSON, because they
sit below the module declaring it or are generic over what they hold,
such as identity's access, a cloud's want and an environment's stage. The developer tools under `tests/` keep their
clap command lines too, under the shapes check's test-code exemption.
All of them are commands and shapes expected to change before they
would earn a declaration of their own.

## Room for Later

What the formats keep for access control.

### Why are some fields kept but unused?

An action's `resource` and a module's `[roles]` [are read and checked
now](actions.md#reserved-fields), though no command acts on them yet. Access control can then build on
declarations modules already have, with no new format version. A specification
field is a parameter of one run, never something access is granted on:
the system that reads it checks it.
