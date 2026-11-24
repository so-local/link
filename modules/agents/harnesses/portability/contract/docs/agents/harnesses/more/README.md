# Harness Contracts

> Contracts and Reference Runtimes

Implement or inspect the boundary between Local Link and a harness.
Adapter commands and endpoints are in [Interface](protocol/README.md), and
the types that callers share are in [Harness Vocabulary](vocabulary/README.md).
The [Agent Protocol](agent-protocol.md) brings the two protocols below
together, so the same agent commands manage different harnesses.

## Exchange Agent State

The [Agent State Protocol](../../../../../agent-state-protocol/docs/agents/harnesses/more/agent-state-protocol.md)
defines portable state exchange across runtimes, separately from the
transport that delivers a prompt.

## Study an Implementation

Study the small reference runtime,
[Mini Harness](../../../../../../mini/docs/agents/harnesses/more/mini/README.md),
to see the model-and-tools loop, how a harness connects to Local Link,
and controlled runtime comparisons.

<!-- docs-tree:start -->

## Docs

### Pages

- [Agent Protocol](agent-protocol.md): The Agent Protocol connects Link to the program running your agent and carries supported state when that program changes
- [Harness Vocabulary](vocabulary/README.md): Share the types a harness and host use without imposing a transport

### More

- [Interface](protocol/README.md): A harness program, `lnk-<name>`, answers a set of commands and speaks typed conversations

<!-- docs-tree:end -->
