# Specifications

Write repeatable configuration in TOML and supply it to a command as a file
or field flags. Each registered kind has one typed reader for both inputs.

## Choose a Kind

The [specification catalogue](../../../../../docs/config/specs/README.md) links to the
reference of every registered kind, from a box to a sandbox.
Each reference shows Every Option and identifies its consuming commands.
The [specification command](../../../../../modules/tooling/spec/docs/tooling/spec/README.md)
lists registered kinds and displays their declared shapes.

## Combine a File and Flags

A consuming command accepts `--spec FILE`. Its field flags override only
the fields named, using the same defaults and validation as the file.
Nested flags follow their TOML key paths, and top-level fields use
`--spec.<field>`. Runtime arguments choose the operation's resource or output
and remain separate from reusable configuration.

## Composed Fields

Lists, maps, variants, and composed specifications retain their declared
types. Each field's reference and command help show the accepted values.
[Specifications and Command Lines](../../../../../docs/more/developing/modules/specs.md) defines the complete
input contract and the checks maintained by each owner.

## Read and Inspect Safely

Relative paths are interpreted from the specification's actual source folder.
Inspection commands show configuration without creating a resource or
rewriting the source. A secret field identifies a secret or its source,
keeping credential values out of process arguments.

## Schemas and Conversion

`schema` identifies the file format. A kind may also have a user-owned
`version`, whose meaning belongs to that kind. Ordinary reads retain the
source, while explicit conversion uses the kind's declared transformations
and refuses ambiguous changes. The [shared specification engine](../libraries/types/specifications.md)
explains conversion and its preservation rules.
