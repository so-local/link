# Tool Actions

An action offered on `tool` may name its MCP call in a `tool` table.
The module's `[tools]` table lists, once, what its program gives at
runtime: each `fills` value a description's `{name}` or an argument's
`x-link-fill` stands for, and each condition a call's `when` names.

```toml
# actions.toml
schema = 1

[tools]
fills = ["timeout", "programs"]
conditions = ["background"]

[actions."run-tool.run.command"]
description = "Runs a program, stopped after {timeout}"
surfaces = ["tool"]
output = "core.mcp"

[actions."run-tool.run.command".tool]
name = "command"
open-world = false
```

## Tool Keys

A `tool` table names the call and its annotations, MCP's hints a host
reads before it calls. Its action binds no specification, since a model's call
carries the action's runtime arguments alone:

- **`name`:** the call's own name, once in the module.
- **`open-world`:** that it reaches beyond the machine, MCP's
  `openWorldHint`.
- **`when`:** the condition it's offered on, one of `[tools]`'s
  `conditions`, or always where left out.

## Annotations

A call's hints come from its action's [effect](actions.md#effects). Its
`readOnlyHint` is whether the effect is `find`. A call that doesn't only
read also says `destructiveHint`, true for `update` and `delete`, and
`idempotentHint`, false only for `insert`. Its `openWorldHint` is given
exactly when its table declares `open-world`, and left out otherwise,
so MCP's default holds.
An argument marked `x-link-summary` is one the call's title shows, and a
boolean one marked `x-link-asks` [makes the call ask the person first](../catalog.md#tools).

## Argument Names a Model Calls

A tool action's argument is declared with dashes, as every argument is
(`start-coordinate`), and its call names it with underscores
(`start_coordinate`), the shape models are trained on. The template, the
call's title and its asks name it so. A command line's flags keep their
dashes.

## Checking a Call's Names

The program's check reads a
call's underscores as the declared dashes, refuses a call that gives the
dashes as an argument it doesn't have, and names each refused argument
as the model called it. Its handler reads the underscores too, and so
does a terminal action's `--request`, the call's own JSON. A declared name never holds an
underscore, so each name a model calls is one argument's. Only a
program's own calls are named so; an action offered as a tool without a
`tool` table keeps its declared names.

## Fills

Braces in a tool action's description, or its arguments', name a fill
and mean nothing else. Every fill and condition `[tools]` declares is
used, and a fill is one kind, text in a description or an argument's
choices. The build renders the module's tool actions as a template with
every `x-link-*` keyword taken out, and the program fills it with
`link_actions::tools::offer`, which refuses a fill or condition the
declarations and the program disagree on.
