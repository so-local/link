# Specification Kinds

`lnk spec` lists every specification kind, shows one's fields and rewrites a file in its current schema.

<!-- quickstart -->

## List the Kinds

A specification is a TOML file that configures one kind of thing, such as an
agent, a box or a sandbox. Each kind belongs to the module that reads
it, which exports the kind's name, its current schema number and its
shape in its description. `lnk spec list` names every kind the
installed modules export, the module that owns it and its schema.

```sh
lnk spec list
```

## Show a Kind

`lnk spec show` prints a kind's fields. Each field comes with its
command-line flag, how to write its value, and what it means. The flag is
the field's key path in the file, such as `--model.thinking`. The command
also names the commands that use the kind and the owner's retired
spellings. With `--json`, it prints the kind's JSON Schema, the standard
description of a document's shape that editors read to check and complete a
file.

```sh
lnk spec show agent
lnk spec show agent --json
```

## Convert a File

A specification file names its kind and its schema at its top, as
`kind = "agent"` and `schema = 1`. When a kind's shape changes, the kind's
owner records each change as a step from one schema number to the next.
Every read applies the steps in memory. `lnk spec convert` rewrites the file
in the current schema, and it is the only command that changes a specification file.

```sh
lnk spec convert agent.toml
```

The file's `kind` finds its owner. Convert applies the owner's exported
steps as data, so converting runs no code of the owning module. Convert
checks the result against the kind's shape before it writes the file. It
keeps the file's comments, key order and permissions, and it refuses a file
that changes while it is read. It also refuses a file that names no kind,
and says how to name one from the kinds installed. Convert never guesses a
kind from what's in the file. A file that is already current only has its
`kind` and `schema` lines moved to the top.

## Find Kinds in Installed Programs

`lnk spec` asks each Link program installed beside `lnk` to describe
itself, the same request `lnk` uses to discover a module. A program
answers before any of its own startup. `lnk spec` names a program that
doesn't answer and passes over it. It refuses two modules that export one
kind.
