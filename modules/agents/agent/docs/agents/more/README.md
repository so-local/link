# More

> More Agent Settings and References

Inspect your agent's configuration and stored files, understand its security boundaries, and integrate with its runtime interfaces. [Agent Specifications](spec/README.md) prints the specification an agent runs.

## Inspect Settings and Identity

[Agent Configuration](settings/README.md) explains startup policy and stored files. [Access](access.md) explains an agent's service identity, and the secrets and team membership attached to the identity.

## Choose Collection Storage

Pick persistence for an agent's collections from the file, database, and bucket drivers in [Stores](../../../../stores/docs/agents/more/stores/README.md). Start there when you implement a driver behind the shared storage interface too.

## Integrate and Diagnose

[Interface](interface/README.md) defines structured commands and runtime protocols. [Security](security.md) describes the boundaries every agent shares, and [Decisions](decisions.md) explains why they work that way. [Troubleshooting](troubleshooting.md) helps you diagnose service and startup failures.

<!-- docs-tree:start -->

## Docs

### Pages

- [Agent Configuration](settings/README.md): An agent's configuration is its specification, which describes the agent, and this machine's settings, which say what any agent here may do
- [Interface](interface/README.md): Integrate with `lnk agent` through structured command output, transfer commands, and requests from other modules
- [Access](access.md): Each agent acts as a service of its own, which is who its secrets are given to and who its teams name
- [Agent Specifications](spec/README.md): An agent's opinions are a specification

### More

- [Decisions](decisions.md): Why shared sandbox policy, proxy mediation and grants work this way
- [Security](security.md): Understand the sandbox boundary, protected machine state and remaining limits shared by every agent
- [Troubleshooting](troubleshooting.md): What to do when your agent doesn't install, start, stay running or answer

<!-- docs-tree:end -->
