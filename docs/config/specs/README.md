# Specifications

> Specifications for Agents and Services

Choose a complete TOML format for an agent, team or cloud resource.
Each reference starts with Every Option and shows the specification before its explanations.
<!-- inventory:specs:start -->

- [Agent Specification](../../../modules/agents/agent/docs/config/specs/agent.md)
- [Box Specification](../../../modules/compute/box/docs/config/specs/box.md)
- [Bundle Specification](../../../modules/tooling/bundle/docs/config/specs/bundle.md)
- [Env Specification](../../../modules/compute/env/docs/config/specs/env.md)
- [Policy Specification](../../../packages/policy/docs/config/specs/policy.md)
- [Sandbox Specification](../../../modules/security/sandbox/docs/config/specs/sandbox.md)
- [Team Specification](../../../modules/agents/agent/docs/config/specs/team.md)

<!-- inventory:specs:end -->

## Shared Definitions

Agents share an `[access]` section that says which people and services may
act on an agent. [Agent Access](../../../modules/agents/agent/contract/identity/docs/config/specs/access.md)
explains the rules.
Some specifications compose access rules and schedule definitions from their
owning types. Their checked references explain those shared fields without
turning each nested definition into a separate specification kind.

<!-- inventory:spec-parts:start -->

- [Agent Access](../../../modules/agents/agent/contract/identity/docs/config/specs/access.md)
- [Schedule Definitions](../../../modules/agents/agent/contract/spec/docs/config/specs/guides/schedules.md)

<!-- inventory:spec-parts:end -->

## Configuration Guides

Learn composition, resource selection, and permission rules in the
[Specification Guides](guides/README.md) before you choose individual fields.

## Composing Specifications

The [Agent Specification](../../../modules/agents/agent/docs/config/specs/agent.md)
references an agent's model, instructions, tools, and connections.
The [Team Specification](../../../modules/agents/agent/docs/config/specs/team.md)
defines a roster, lead, and settings shared by its members.

### Machines and Environments

Use the [Box Specification](../../../modules/compute/box/docs/config/specs/box.md)
for a cloud machine's resources and attachments, and the
[Env Specification](../../../modules/compute/env/docs/config/specs/env.md) for the
system, setup stages, and services prepared on a machine.

### Permissions and Policies

The [Sandbox Specification](../../../modules/security/sandbox/docs/config/specs/sandbox.md)
references grants on files, programs, and network access. The
[Policy Specification](../../../packages/policy/docs/config/specs/policy.md) describes
a principal's allowed actions and resources.

### Shared Design Rules

Every specification follows common rules. [Specification Decisions](decisions.md) explains
them and the rationale behind the individual formats.

### Composition Guides

The [Agent Specification Guide](../../../modules/agents/agent/contract/spec/docs/config/specs/guides/agent.md)
explains how an agent's specifications meet as sets. The [Team Specification Guide](../../../modules/agents/agent/contract/team/docs/agents/more/interface/contract/team.md)
explains how a team's file makes its roster. The [Box Specification Guide](../../../modules/compute/box/docs/config/specs/guides/box.md)
explains how a box specification picks its machine. The [Sandbox Specification Guide](../../../modules/security/sandbox/docs/config/specs/guides/sandbox.md)
explains what a sandbox specification opens and refuses. The [Policy Specification Guide](../../../packages/policy/docs/config/specs/guides/policy.md)
explains a policy's actions and resources. The [Bundle Specification](../../../modules/tooling/bundle/docs/config/specs/bundle.md)
says which programs a release of your own builds.

## Agent Behavior

An agent's [schedule definitions](../../../modules/agents/agent/contract/spec/docs/config/specs/guides/schedules.md)
choose when work runs, and its [connection policy](../../../modules/agents/connections/contract/policy/docs/agents/connections/more/protocol/policy.md)
sets which messages it answers.

<!-- docs-tree:start -->

## Docs

### Pages

- [Specification Guides](guides/README.md): Specification guides explain how each resource's specification fits together

### More

- [Decisions](decisions.md): Why Link's specifications are what they are: first what holds for every specification, then each one's own

<!-- docs-tree:end -->
