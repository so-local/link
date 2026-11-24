# Types File

`types.toml` holds a module's shapes, its resources, specification kinds and
outputs among them, each named once and written in a subset of JSON
Schema that maps to one type in Rust and TypeScript.

```toml
# types.toml
schema = 1

[stage]
type = "object"
description = "One step an environment runs"
additionalProperties = false
required = ["run"]
properties.run = { type = "string", description = "The command it runs" }
properties.timeout = { shape = "core.duration", default = "1m", description = "How long it may take" }
```

## Named Shapes

Each top-level table is one shape, and its key is its name. Wherever a
schema goes, a shape's name may stand instead: `items = "stage"` is a
list of the module's `stage`, and `"box.id"` [a shape of the `box`
module](../catalog.md#names), which `module.toml` must list as a dependency. A name is never a file or a path, so
shapes can move between files and folders without a reference changing.

### Names Where They Stand

A property naming a shape may say more of itself where it stands, as
`{ shape = "core.duration", default = "1m", description = "How long it
may take" }`, with its own `description`, `examples` and `default`. It
renders as the name's reference beside its own keywords, and generates
as the named type with its default applied. TOML has no null, so a value
that may be left out is one its record doesn't require.

### Resources

A resource is a shape an action ID names, as `approval` in
`env.approval.add`. Its `x-link-resource` says which property is its
ID, and the actions on it [take their output from it](actions.md#outputs).
A resource is a record.

## Subset

`link-actions` reads only the part of JSON Schema that maps one for one
to a type, and refuses every other keyword by name, `$ref`, `$defs`,
`allOf`, `anyOf`, `not`, `if` and `patternProperties` among them. The
shapes it reads:

- **Record:** `type = "object"`, its `properties` and `required`, and
  `additionalProperties = false`.
- **Map:** `type = "object"` with `additionalProperties` a schema or a
  name. Its keys, where typed, are a text shape in `propertyNames`, and
  it's generated ordered by that key.
- **Open record:** a record's `properties` with `additionalProperties`
  a schema or a name, generated as the named fields and a flattened map,
  `others`.
- **List:** `type = "array"` and its `items`.
- **Choice of values:** `enum` or `const`.
- **Choice of records:** `oneOf`, told apart as [Choices](#choices)
  says.
- **Choice by value:** `oneOf` of at most one number, one record and one
  text, and words, told apart by the value, as a size is `8`, `"8 GB"`
  or `{ min, max }`. A word is read before the text.
- **Scalar:** `string`, `boolean`, `integer` or `number`, with a
  standard `format` or [one of Link's](#links-formats).
- **Any value:** `x-link-any = true`, alone, JSON Schema's `{}`, only
  for a value Link carries and doesn't define, as a tool call's
  arguments.

### Bounds

Validation keywords that map to a check are read and enforced by the
generated type: `minimum`, `maximum`, `minLength`, `maxLength`,
`pattern`, `minItems` and `maxItems`. Bounds that can't hold anything
are refused.

### Link's Formats

Four formats are Link's own, each read into its own type: `duration`,
as `30s`, `size`, as `16 MB` in B, KB, MB or GB, each a power of two,
`rate`, as `10/s` per s, m or h, and `seal-to`, the public half of an
X25519 key in base64, which a secret is sealed to on its way to the
key's holder. Other standard formats, such as
`date-time`, describe a string and aren't checked.

### Choices

A choice of records is told apart by a tag, one `const` property naming
the kind each choice is, the same property in every choice. Where a tag
would only repeat what one required key says, a choice may instead be
told apart by a marking key each choice requires and no other has. The
declaration names each choice's key in order in `x-link-markers`, so a
reader never infers a type from which values happen to be present.

```toml
# types.toml
schema = 1

[step]
oneOf = ["stage", "group"]
x-link-markers = ["run", "parallel"]
```

## Link's Keywords

A shape may carry keywords of Link's own, each starting `x-link-`:

- **`x-link-spec`:** [this shape is a specification kind](#specifications), with its kind's name,
  unique across Link.
- **`x-link-resource`:** this shape is a resource, and which property
  is its ID.
- **`x-link-kind`:** this string is an ID of the kind named,
  `<kind>:<id>`, generated as a typed ID. On a runtime argument, a
  program may look up a name typed for it on the command line: it gives
  `link_actions::cli::names` a lookup for the kind before parsing, and
  the parser sends the ID the lookup finds, so no action receives a
  name. A module looks a name up among the things of that kind you have.
- **`x-link-path`:** this string is a path, resolved from the file's
  folder or the current folder. `"confined"` also keeps it inside the
  specification's own folder.
- **`x-link-ref`:** this string, or a map's keys where it's on
  `propertyNames`, names a `module`, a `command` or a `tool`, [read as
  the name it has today](../../../../../../spec/docs/more/libraries/types/specifications.md#names-that-follow-history).
- **`x-link-sensitive`:** this value is a secret, generated as a
  zeroized `Secret` that's never printed. A field flag never takes its
  value.
- **`x-link-shorthand`:** on an object, the property a bare value on
  the object's own flag sets, so `--engine v8` is
  `--engine.name v8`; an inline table still sets the whole
  object.
- **`x-link-values`:** what each of an enum's words means, generated as
  the enum's `meaning`.
- **`x-link-checks`:** the [hooks](../catalog.md#hooks) this shape's
  value is checked by too, run inside its generated `Validate`.
- **`x-link-markers`:** each choice of a `oneOf` by its marking key.
- **`x-link-unavailable`:** why a run refuses a value given here, so one
  specification reads alike everywhere.
- **`x-link-display`:** how a person sees this shape where its verb's
  way doesn't suit.
- **`x-link-rust`:** a part of a hidden argument's payload the format
  can't say yet, named by its Rust type's whole path
  (`::link_sandbox_route::Route`) and read with that type's own serde.
  Every surface shows it as any value. It is the one place a Rust type
  is a shape's source, so each declaration using it is on the shapes
  check's own list, which only shrinks.

### Tool Keywords

Three keywords are [a tool action's own arguments'](tools.md).
`x-link-summary` places an argument in the call's title, by its place
from 1, and never on a sensitive one. `x-link-fill` names the fill whose
choices its program gives, with no `enum` of its own. `x-link-asks`
makes a boolean argument ask the person first unless it's given false.

## Specifications

A module may declare several specification kinds. The kind named as its namespace
keeps its versions and steps in `schema.toml`, and another kind in
`<kind>-schema.toml`, as a `profile` kind's `profile-schema.toml`, each read as
[Transformation
Steps](../../../../../../spec/docs/more/libraries/types/specifications.md#transformation-steps)
says. A specification file names its `kind` and `schema`, which take no flag, and
its shape's `default`s apply to the file and its flags alike.

## Generated Types

Each shape [becomes a Rust type](../../../../../build/docs/more/developing/modules/catalog/generation.md#types) that enforces what the shape says, so no
handler sees an unchecked value.
A whole number is the smallest Rust integer its bounds fit, unsigned
from a least of 0. A property left out takes the default it declares,
as `{ shape = "isolation", default = {} }` for a table whose every field
may be left out. A property declaring none is optional in Rust, since a
section left out isn't an empty one. A rule across fields is a named
check hook.

### Empty Defaults

A default of nothing, an empty list or `{}`, is never written back: a
value left at it prints as the property left out, so a printed specification
reads back the same.
