# Tooling

> Tooling Around Your Agents

Tooling gives you tools that run around your agents. You can measure what
each one costs, read this manual offline, and build Local Link's programs
into one signed folder with [Bundle](../../bundle/docs/tooling/bundle/README.md).
Each tool is a module of its own.

## Move Agent State in an Archive

An [archive](../../archive/docs/tooling/archive/README.md) carries your
agents' state between machines and people in one file. It only reads and
writes files, so you can share an agent or start one that someone else
made.

## Read the Specification Kinds

Agents, boxes, sandboxes and the rest are configured by specifications, which are
TOML files that each module reads. The [Specification](../../spec/docs/tooling/spec/README.md)
module lists every kind the installed modules export, shows the fields of
one kind, and converts an old file to its current schema.

## Measure Costs and Read the Manual Offline

[Measure](../../measure/docs/tooling/measure/README.md) shows current
resource use and the costs it kept. [Manual](../../manual/docs/tooling/manual/README.md)
reads the installed documentation offline. The [Tooling Overview](guide.md)
shows how these tools fit around an agent.

<!-- docs-tree:start -->

## Docs

- [Tooling Overview](guide.md): What Link runs around your agents: what each agent costs, and these docs offline

<!-- docs-tree:end -->
