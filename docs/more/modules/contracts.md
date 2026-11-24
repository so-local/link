# Module Contracts

A module exposes a versioned interface. Each subject owns its wire
shapes and client behavior, on the page of the crate that owns it.

## What a Contract Means for You

A contract is why modules you install at different times still work
together. Every module of one release speaks the same versions, and
`lnk upgrade` moves them together. A module left behind by an
interrupted upgrade, or copied in from another release, is refused by
its name, with [`lnk upgrade`](guide.md#upgrade) as the fix. A module
from outside Link works with any `lnk` that accepts the versions it
speaks. [What a refusal names](../../../core/framework/docs/more/modules/protocol.md#what-a-refusal-names)
shows that refusal.

## Commands

Examples of command groups with documented JSON output follow. The
[command index](../reference/commands.md) lists the complete declared
command set, while each owning guide explains its output.

- [`lnk agent`](../../../modules/agents/agent/docs/agents/README.md#agents)
- [`lnk measure`](../../../modules/tooling/measure/docs/tooling/measure/README.md#measure)
- [`lnk model`](../../../modules/agents/model/docs/agents/models/README.md#models)
- [`lnk storage`](../../../modules/storage/storage/docs/services/storage/README.md#storage)
- [`lnk cloud`](../../../modules/cloud/cloud/docs/services/cloud/README.md#cloud-accounts)
- [`lnk box`](../../../modules/compute/box/docs/services/boxes/README.md#boxes)

## Agent Interfaces

How an agent moves, and how an account hosts a team:

- [Moving an agent](../../../modules/agents/agent/docs/agents/more/interface/README.md#moving-an-agent)
- [Hosted teams](../../../modules/agents/agent/contract/place/docs/agents/more/interface/contract/hosting.md#hosted-teams)

## Confinement and Secrets

How a program is confined, and the secrets it is handed:

- [`lnk sandbox`](../../../modules/security/sandbox/docs/security/sandbox/README.md#sandbox)
- [Secrets a program is handed](../../../modules/security/secret/docs/config/secrets/more/protocol.md#secrets-a-program-is-handed)

## Protocols

What each kind of module answers, by its kind:

- [Adding and removing modules](../../../core/framework/docs/more/modules/protocol.md#adding-and-removing-modules)
  - [`lnk mod`](guide.md#add-or-remove-a-module)
  - [Uninstalling (`lnk-<module> uninstall`)](../../../core/framework/docs/more/modules/protocol.md#uninstalling-lnk-module-uninstall)
- [Harness programs (`lnk-<name>`)](../../../modules/agents/harnesses/portability/contract/docs/agents/harnesses/more/protocol/README.md#harness-programs-lnk-name)
- [Memory engines (`lnk-<engine>`)](../../../modules/agents/tools/memory-tool/contract/docs/agents/tools/memory/more/protocol.md#memory-engines-lnk-engine)
- [Bridges (`link-sandbox-bridge`)](../../../modules/security/sandbox/bridge/docs/security/sandbox/more/bridges.md#bridges-link-sandbox-bridge)
- [Store drivers (`lnk-<name>`)](../../../modules/agents/stores/contract/docs/agents/more/stores/protocol.md#store-drivers-lnk-name)
- [Connection modules (`lnk-<connection>`)](../../../modules/agents/connections/contract/docs/agents/connections/more/protocol/README.md#the-contract)
- [Cloud Adapters](../../../modules/cloud/cloud/contract/docs/services/cloud/more/adapters.md)
- [Typed Link conversations](../../../modules/agents/harnesses/portability/contract/docs/agents/harnesses/more/protocol/conversations.md#typed-link-conversations)
