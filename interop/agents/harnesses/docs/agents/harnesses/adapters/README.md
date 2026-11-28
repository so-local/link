# Harness Adapters

> Harness Adapters Put You in Control

Run a third-party harness through Local Link's agent commands, with your model, files, tools, and messaging connections. Each adapter installs and controls its harness. Each adapter also declares which existing installations and state transfers it supports.

## Create an Agent on Another Harness

Create an agent on a chosen harness when you want its native tools and behavior under Local Link's management. DeepSeek, Hermes, and OpenClaw support this route. [Compare Harnesses](../compare.md) separates model choices, management, migration, and transfer capabilities so you can choose for your task.

```sh
lnk agent create work --harness deepseek|hermes|openclaw
```

## Migrate an Existing Installation

An installation you already use has native history, configuration, credentials, and possibly background services. [Migrate to Local Link](../../../../../../../modules/agents/agent/docs/agents/migrate/README.md) brings supported OpenClaw, Hermes, and DeepSeek installations under Local Link.

```sh
lnk agent migrate --harness openclaw|hermes|deepseek
```

## Manage a Harness

Local Link uses each adapter to configure the harness, start and stop it, check whether it answers, and connect its messages and tools. The [Agent Protocol](../../../../../../../modules/agents/harnesses/portability/contract/docs/agents/harnesses/more/agent-protocol.md) explains the shared control and state model. Your harness keeps its native agent loop and features, subject to the permissions and configuration its guide describes.

## Understand State Transfers

Switching the harness, moving the machine, and adopting an installation have different state requirements. The comparison distinguishes native conversation import, archive-backed continuation, native-data moves, and installation migration. A harness can be useful for fresh creation and control even when it cannot import another harness's history.

## Choose an Adapter

Each adapter's guide explains how to start, what Local Link configures, what transfers, and which native behavior stays with the harness. Each adapter's decisions page explains the work behind those choices. [Integration Decisions](decisions.md) explains why Local Link focuses on personal agents, and why the shared protocol and the individual adapters have different jobs.

- [DeepSeek Harness](../../../../deepseek/docs/agents/harnesses/deepseek/README.md) has native coding tools and persistent sessions with portable continuation.
- [Hermes Agent](../../../../hermes/docs/agents/harnesses/hermes/README.md) has native sessions, skills, and memory, which its adapter translates.
- [OpenClaw](../../../../openclaw/docs/agents/harnesses/openclaw/README.md) has personality and memory files with gateway-backed sessions.

## Add Another Harness

Contributors can improve one adapter or add a harness through the same [adapter interface](../../../../../../../modules/agents/harnesses/portability/contract/docs/agents/harnesses/more/protocol/README.md) and conformance kit. [Adding a Harness](../../../../../../../docs/more/developing/modules.md#adding-a-harness) gives the implementation route. Declare control and transfer capabilities independently, so users can understand and use each part you deliver.

## Implement the Agent Protocols

Harness creators can expose the Agent Client Protocol (ACP) for sessions and control, and Local Link's optional Agent State Protocol (ASP) for supported state transfer. Direct protocol support lets an adapter delegate those operations to the harness. Native representations, permissions, and unsupported state remain explicit in the integration.

<!-- docs-tree:start -->

## Docs

- [Decisions](decisions.md): Why Link integrates third-party harnesses through shared contracts while keeping their native differences in adapters

<!-- docs-tree:end -->
