# Agent Contract

> Agent Forms Shared Across Boundaries

The agent contract gives callers one definition of an agent's folders, identity, namespace actions, and transfer stream. Use the contract when another program needs to inspect or move an agent without starting its harness.

## Move an Agent

Senders and receivers share the transfer envelope, manifest, hashes, and collection records that [Move Stream](move-stream.md) defines. Accounts that host agents use the requests and replies that [Hosting Protocol](../../../../../place/docs/agents/more/interface/contract/hosting.md) defines.

## Read Configuration

[Agent Access](../../../../../identity/docs/config/specs/access.md) says who may message an agent and how a sender's standing limits actions. The [Agent Specification](../../../../../../docs/config/specs/agent.md) lists an agent's opinions, and the [Team Specification](../../../../../../docs/config/specs/team.md) lists a team's roster and shared settings. The [Team Specification Guide](../../../../../team/docs/agents/more/interface/contract/team.md) explains how a team's file makes its roster and what its members share, as `link-agent-team` reads the file.

## Integrate Actions

The agent module's [Action Namespaces](../../../../../../docs/agents/more/interface/namespaces.md) describe the command, tool, and route surfaces over these types. The module owns dispatch and running services, and the contract supplies the forms they exchange.

<!-- docs-tree:start -->

## Docs

- [Move Stream](move-stream.md): The stream an agent moves in, as every end of it reads and writes it: `lnk agent state-send` and `receive`, a cloud that hosts agents (`agent-receive`, `agent-send`), and `lnk storage` for such an agent's Home…

<!-- docs-tree:end -->
