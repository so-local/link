# Specification Cases

Specification Cases holds each specification's cases, one TOML file per case, giving a specification as a person writes it (`[spec]`) and what must come of it (`[expect]`). Each specification's own tests read its folder, so a new case is a new file.

<!-- docs-tree:start -->

## Docs

- [Agent Specification Cases](agent/README.md): `link-agent-spec`'s `tests/specs.rs` reads the agent specification's cases from this folder
- [Box Specification Cases](box/README.md): The box module's `tests/box/specs.rs` reads the box specification's cases from this folder
- [Environment Specification Cases](env/README.md): The env module's `tests/env/specs.rs` reads the environment file's cases from this folder
- [Policy Specification Cases](policy/README.md): `link-policy`'s `tests/specs.rs` reads the Policy Specification's cases from this folder
- [Sandbox Specification Cases](sandbox/README.md): The sandbox module's `tests/specs.rs` reads the sandbox specification's cases from this folder

<!-- docs-tree:end -->
