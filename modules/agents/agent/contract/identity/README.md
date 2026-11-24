# Agent Identity

`link-agent-identity` holds agent IDs, owners, and who outside may do what, without a running agent. Every specification shares the package's access type.

<!-- docs-tree:start -->

## Docs

- [Agent Access](docs/config/specs/access.md): `[access]` says who besides its owner may do something with the agent, each verb a list: chats on its connections, which are groups, or principals, `service:<id>`, written as Link's access tokens write who acts

<!-- docs-tree:end -->

## Code

- [`access.rs`](src/access.rs): who outside may do what
- [`lib.rs`](src/lib.rs): names, ids and where an agent is, from the machine's and the secrets' crates
- [`owner.rs`](src/owner.rs): the grammar of an owner
