# Shared Types

`link-types` holds core's shapes, the ones every module shares, generated
once from core's declarations so each has one home and one Rust type.

## Declared Once

Core's declarations are in `link-actions`' `core/` folder, a module whose
ID is `core`, and every catalog loads them. A module names one of its
shapes as `core.<name>`, with no dependency in its `module.toml`, and its
generated code holds this crate's type:

```toml
# types.toml
schema = 1

[checked]
type = "object"
required = ["os"]
additionalProperties = false
properties.os = "core.os"
```

## The Shapes

Core declares six shapes today. The first four are single values a specification
or a command holds, and the last two are answers an action names as its
`output` in place of a typed reply:

- **`core.os`** is `Os`, a machine's OS, which a box and an environment
  each name: `ubuntu-24.04`, `ubuntu-22.04`, `debian-13` or `macos`.
- **`core.arch`** is `Arch`, a machine's processor: `x86_64` or `arm64`.
- **`core.user`** is `User`, who a run runs as on a box: `root`, outside
  any sandbox, or a user of the box by name.
- **`core.duration`** is `Duration`, how long, written as a whole number
  and `ms`, `s`, `m`, `h` or `d`.
- **`core.report`** is `Report`, what a command said: the blocks it
  printed, as `link-format` writes them, and its exit code.
- **`core.mcp`** is `Mcp`, an MCP (Model Context Protocol) tool call's
  result, relayed as the tool gave it: its content blocks, and its
  structured content and error flag where it gives them.

### Methods

Each keeps the methods its users had: an OS's word and default, a
processor's word and default, whether a user is root and its name, and a
duration's length. `link-machine`, the
sandbox's box users and an environment's timeouts name these types
where they named their own before.

## Reading Checked

Hand-written specifications hold these types too, so each is checked as it's read,
not only by a generated `Validate`: a user who isn't `root` or a valid
name is refused while the file is read. Each also describes itself as
JSON Schema, from the catalog, for a hand-written specification's reference.
