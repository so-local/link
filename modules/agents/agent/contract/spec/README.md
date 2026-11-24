# Agent Specification

`link-agent-spec` defines an agent's specification, the TOML file of what the agent is and its opinions. The package also resolves the agent's team's opinions beneath the agent's own. The agent module and every harness read the specification.

<!-- docs-tree:start -->

## Docs

- [Agent Specification Guide](docs/config/specs/guides/agent.md): How the specifications an agent runs meet, where each is kept and how its values are written
- [Schedule Definitions](docs/config/specs/guides/schedules.md): `schedules` holds the agent's schedules: what fires when, and where it goes
- [Features](docs/config/specs/guides/features.md): What an agent can do, each feature by name, switched by its own keys of the agent specification

<!-- docs-tree:end -->

## Code

- [`agent_spec.rs`](src/agent_spec.rs): the specification and its tables
- [`lib.rs`](src/lib.rs): the crate and what it carries
