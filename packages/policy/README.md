# Policy

`link-policy` decides who may do what. It holds the Policy Specification and the one check of a principal's action on a resource, where a principal is the user or service that acts.

<!-- docs-tree:start -->

## Docs

### Pages

- [Policy Specification Guide](docs/config/specs/guides/policy.md): What a policy's actions, resources and principals are, how roles and a resource's `[access]` use one, and where its boundaries are

### References

- [Policy Specification](docs/config/specs/policy.md): A Policy Specification: an API key's scopes, a role, an agent's grants, a host's guest's

<!-- docs-tree:end -->

## Code

- [`check.rs`](src/check.rs): `allows`, the one check
- [`made.rs`](src/made.rs): what each principal made, ended with it
- [`name.rs`](src/name.rs): principals, the action list and resources
- [`policy.rs`](src/policy.rs): the Policy Specification, its limits and `[access]`
- [`role.rs`](src/role.rs): roles, and who holds each
