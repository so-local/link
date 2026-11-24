# Agent Team

`link-agent-team` reads a team from its file. The package covers the team, its members, the folder they share, and the agents of other accounts the team lets on.

<!-- docs-tree:start -->

## Docs

- [Team Specification Guide](docs/agents/more/interface/contract/team.md): How a team's file makes its roster, what its members share and how other accounts' agents join

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): a team and its members
- [`moving.rs`](src/moving.rs): whether a member moved alone keeps its team's guarantees
