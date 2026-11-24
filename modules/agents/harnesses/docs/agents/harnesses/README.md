# Harnesses

> Harnesses That Run Your Agent

A harness is the program that runs your agent. It talks to the model,
keeps the conversation, and calls tools. You choose a supported harness,
and Local Link installs it, keeps it in a sandbox and up to date, and
lets you switch any time.

<!-- quickstart -->

## List the Harnesses

List the available harnesses and see which one runs your agent. The
[harness guide](guide.md) explains their differences and requirements.
You can also [compare four personal harnesses](../../../../../../interop/agents/harnesses/docs/agents/harnesses/compare.md)
side by side, or read about
[OpenClaw](../../../../../../interop/agents/harnesses/openclaw/docs/agents/harnesses/openclaw/README.md),
[Hermes Agent](../../../../../../interop/agents/harnesses/hermes/docs/agents/harnesses/hermes/README.md),
and [DeepSeek Harness](../../../../../../interop/agents/harnesses/deepseek/docs/agents/harnesses/deepseek/README.md).

```sh
lnk agent harness list
```

## Choose Link Harness or Mini Harness

Link Harness is the default personal harness. It has instructions,
skills, and richer conversation management. [Mini Harness](../../../mini/docs/agents/harnesses/more/mini/README.md)
is Local Link's smallest harness. You use Mini Harness to study the model-and-tools
loop, to check how a harness connects to Local Link, and to measure
benchmark baselines.

## Switch Your Agent to Another Harness

Switch your current agent to OpenClaw, installing it if needed. The
[harness guide](guide.md) explains what is kept when you switch and how
to switch back.

```sh
lnk agent harness set openclaw
```

## Create an Agent on a Harness

Create a separate agent named `work` on Hermes. The [harness guide](guide.md)
covers choosing a harness for a new agent.

```sh
lnk agent create work --harness hermes
```

## Move Between Harnesses

You can change an existing agent's harness and keep its state, as
[Switch Harnesses](../../../../agent/docs/agents/harnesses/use/README.md)
explains. To bring a native installation under Local Link first, follow
[Migration](../../../../agent/docs/agents/harnesses/use/migrate/README.md).

## Add a Harness

Third-party harnesses run through [adapters](../../../../../../interop/agents/harnesses/docs/agents/harnesses/adapters/README.md),
and you can write your own. The [Agent Protocol](../../../portability/contract/docs/agents/harnesses/more/agent-protocol.md)
shows how Local Link controls a harness and moves its state.
[Harness Contracts](../../../portability/contract/docs/agents/harnesses/more/README.md)
holds the protocols, the shared vocabulary, and the small reference
implementation.

### Protocol Details

The [Harness Interface](../../../portability/contract/docs/agents/harnesses/more/protocol/README.md)
defines how an adapter installs, configures, and reaches a harness. The
[Agent State Protocol](../../../portability/agent-state-protocol/docs/agents/harnesses/more/agent-state-protocol.md)
defines how state moves between harnesses, whatever each one stores natively.

<!-- docs-tree:start -->

## Docs

- [Harnesses Guide](guide.md): What Link does for whichever harness runs your agent, what each one needs, and how to add one from outside Link

<!-- docs-tree:end -->
