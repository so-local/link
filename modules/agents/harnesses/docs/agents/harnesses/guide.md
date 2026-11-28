# Harnesses Guide

**What Link does for whichever harness runs your agent, what each one
needs, and how to add one from outside Link.** [Harnesses](README.md)
introduces harnesses and the commands that list, set, and create
them, and [Switch Harnesses](../../../../agent/docs/agents/harnesses/use/README.md)
says what comes along when you switch.

## Compare them

[Compare Harnesses](../../../../../../interop/agents/harnesses/docs/agents/harnesses/compare.md)
separates fresh creation, installation migration, conversation transfer,
and native-data moves for Link Harness, Mini, and the third-party
adapters. [Harness Adapters](../../../../../../interop/agents/harnesses/docs/agents/harnesses/adapters/README.md)
introduces the adapters and how to contribute another.

## What Link does for every harness

The [Agent Protocol](../../../portability/contract/docs/agents/harnesses/more/agent-protocol.md)
gives harnesses common conversation and supported state operations.
Each adapter joins those operations to Link's management commands.
Their guides declare model requirements, permissions, and transfer
limits, so common management does not imply identical native behavior.

- Link installs the selected release and keeps mutable state in its
  own home at `~/.lnk/data/agents/<agent>/harnesses/<name>`. Each guide
  describes its program layout. [`lnk upgrade`](../../../../../../docs/more/modules/guide.md#upgrade)
  updates the adapters Link comes with along with Link.
- It sets the harness up with your model, your files folder, your
  connections and your tools, written into the harness's own settings.
- It sandboxes the harness. The harness writes only its own folders and
  reaches the internet only through Link's filtering proxy, and [it asks
  you before it gets anything more](../../../../agent/docs/agents/harnesses/use/harness-permissions/README.md#allow-what-a-harness-needs).
- It configures the model route and credential handling the adapter
  supports. The adapter's guide and security page describe whether
  the harness receives a credential or Link's proxy adds it on the wire.
- It connects your phone with [`lnk agent connect telegram`](../../../../agent/docs/agents/connections/connect/README.md#choose-an-app), and [only you
  can talk to the agent](../../../../connections/docs/agents/connections/README.md).
- It makes [Link Memory](../../../../tools/memory-tool/docs/agents/tools/memory/README.md)
  available through the shared tools, searching the agent's files and
  conversation records exposed to Link.
- It moves the agent. [`lnk agent move`](../../../../agent/docs/agents/move/README.md#move-your-agent-to-the-cloud)
  carries [shared files and the adapter's declared native state](../../../../agent/docs/agents/move/README.md#move-your-agent-to-the-cloud).
- It checks the harness. [`lnk agent harness check`](../../../../agent/docs/agents/harnesses/use/harness/README.md#inspect-the-sandbox) tries, from inside
  the sandbox, what the harness shouldn't be able to do.

## Refresh Native Tool Routes

The [Hermes](../../../../../../interop/agents/harnesses/hermes/docs/agents/harnesses/hermes/decisions.md#why-refresh-tool-routes-separately) and [OpenClaw](../../../../../../interop/agents/harnesses/openclaw/docs/agents/harnesses/openclaw/decisions.md#why-refresh-tool-routes-separately) adapters refresh tool and memory addresses on every configure because the agent’s proxy can assign new endpoints. They update native files without repeating structural CLI configuration for an address-only change, and save the configuration fingerprint only after those writes succeed.

## What they need

You need the agents modules, which [`lnk up`](../../../../../../docs/more/modules/install.md#pick-your-modules) adds. Every harness
here comes with Link and is free. The chosen adapter determines
which model interfaces it supports, and a hosted [model](../../../../agent/docs/agents/models/agent/README.md) bills you.

## Use another harness

A harness from outside Link comes as a module with an adapter that
answers the harness contract. Add it once, then switch to it by its
name: [Switch Harnesses](../../../../agent/docs/agents/harnesses/use/README.md#use-another-harness).

## Why it works this way

[Agents Decisions](../../../../agent/docs/agents/more/decisions.md) says why Link runs every harness
this way, and [Decisions](../../../portability/contract/docs/agents/harnesses/more/protocol/decisions.md) why a harness meets Link through its contract.
