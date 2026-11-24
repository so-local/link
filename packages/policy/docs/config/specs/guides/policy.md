# Policy Specification Guide

What a policy's actions, resources and principals are, how roles and a
resource's `[access]` use one, and where its boundaries are. Each field
is in the [Policy Specification](../policy.md) reference. In this `ci.toml`, each
`[[allow]]` is a grant of every action it names on every resource it
names. `[limits]` says how much: two boxes at once, each with at most 4
CPUs, 8 GB of memory and a 40 GB disk, living at most 12 hours, and each
limit left out is none but the service's own.

```toml
# ci.toml
schema = 1
name = "ci"
version = 1

[[allow]]
actions = ["cloud.read", "cloud.use"]
on = ["cloud:aws", "cloud:ci-*"]

[[allow]]
actions = ["box.*"]
on = ["box:ci-*"]

[limits]
boxes = 2
cpus = 4
memory_gb = 8
disk_gb = 40
hours = 12
```

A policy holds at most 64 grants. A key it doesn't have is refused by
its name, as an empty table is.

## Actions

An action is `<kind>.<verb>`, from one list: a verb the list doesn't
hold can't be granted or checked. Three verbs a kind: what's read,
what's used, what changes it.

### Agent actions

`agent.read` reads an agent's status and logs, `agent.message` messages
it (a teammate's message, a post to its webhook), and `agent.admin`
starts, stops, moves it or changes its specification.

### Team actions

`team.read` reads a team, `team.member` is on it (its chats, its
folder, its specification), and `team.admin` changes who's on it and what each
reaches.

### Bucket actions

`bucket.read` reads and lists a bucket or a path of it, `bucket.write`
writes there, and `bucket.share` shares it. A path is the resource's id
after the bucket's, `bucket:team-r7/drafts/`, and a pattern reaches what
it begins, `bucket:team-r7/drafts/*`.

### Cloud actions

`cloud.read` reads a cloud's boxes and limits. `cloud.use` has boxes
made on it within those limits, and `cloud.admin` accepts it and changes
who may use it.

### Box actions

`box.read` reads a box, `box.ssh` reaches it over SSH, and `box.admin`
makes, deletes or resizes it. These verbs apply to box resources rather
than cloud resources.

### Grant patterns

A grant's `actions` takes an action, `<kind>.*` (every verb of a
kind), or `*` (every action, the owner's). Its actions and its
resources are of one kind: `box.ssh` on `cloud:aws` is refused.

## Resources

A resource is named by its id, `<kind>:<id>`; another account's is
`<account>/<kind>:<id>`, as in `acct-7f3a/cloud:mac`. A name without an
account is the principal's own account's.

### Resource patterns

A grant's `on` takes a name or a pattern. A `*` ends a pattern or is
one; nothing else is a pattern, so a grant reads as what it reaches.

#### One resource

`cloud:aws` reaches that cloud alone. `cloud:ci-*` reaches every cloud
whose id begins `ci-`, within the principal's account unless an account
is named explicitly.

#### Clouds of one or every account

`cloud:*` reaches every cloud of the principal's account. `*/cloud:*`
reaches clouds of any account where the resource's `[access]` lets the
principal in.

#### Every resource of the account

`*` reaches everything of the principal's account. Cross-account access
still needs an account-qualified pattern and the resource's own
`[access]` permission.

## Principals

Each principal has a permanent id, `<kind>:<id>`, its id after its kind
of the characters `A-Z a-z 0-9 . _ : / -`, at most 128, and its kind
one of two.

### Users

`user:` identifies a person: you, signed in, or someone a connection
vouches for. A machine is never a principal: its signer signs for its
user, so what a machine does, its user does.

### Services

`service:` identifies a program acting on its own. An agent acts as a
service for its lifetime. A caller that creates a temporary service principal
supplies its expiry when constructing the checked principal.

### Ends

A principal may end: the service that holds it says when, and a check
after then is refused.

## Roles

A role is a Policy Specification given to principals. `Roles` keeps its grants and
holders together, and the service loads those grants into a principal
snapshot before authorizing an operation. Updating the role changes newly
constructed snapshots rather than mutating existing ones.

### Role Names and Patterns

A role's `name` uses lowercase letters, digits, and `-`, beginning with a
letter and at most 64 characters long. Renaming creates a new role. Patterns
on agents, teams, or buckets end a segment with `*`, alone or after `/`,
such as `bucket:team-r7/*`. The decision identifies the matching role and
grant, so a caller can explain why access was allowed.

## A resource's `[access]`

The resource keeper supplies `Access` when constructing a checked `Resource`.
Its TOML representation maps a verb of the resource's kind to a list: `*` (anyone), an account (`acct-<id>`, every principal of it) or
a principal. A cloud's `[access]` with `use = ["acct-7f3a"]` lets every
principal of that account have boxes made on it, when their own
policy allows it too. Inside the account, the principal's policy alone
decides, and `[access]` isn't read.

## Caller Responsibilities

`allows` checks the supplied principal, action, resource, and time without
reading a store or reaching a service. Authenticate the principal and obtain
current resource ownership and access rules before constructing those inputs.

### Refresh Role Snapshots

Looking up a principal's roles returns copies of their policies, and the
principal keeps those copies. A role update does not mutate existing principals.
Reconstruct the snapshot from current roles when an operation must observe
revocation or changed grants.

### Enforce Limits Separately

The authorization decision does not meter `[limits]`. The resource keeper
supplies projected `Usage` to the policy's limits check and enforces the result while
serializing resource allocation, so simultaneous requests cannot bypass a
quota by each observing the same earlier usage.

### Whole-Pattern Grants

`allows_all` checks whether a grant covers the whole requested pattern within
the principal's account. `allows_all_with_access` can check one explicitly
named other account using access obtained from that resource's keeper.
Neither accepts an unrestricted any-account pattern as proof of access.

## Policy boundaries

A policy states grants and limits. Its principal and the service that
holds it determine when it ends and who receives it; the policy keeps
no separate deny or conditional rule system.

- **No deny rules.** What isn't granted is refused; a narrower
  principal is made rather than a wider one denied.
- **No conditions on a grant.** No address, time or hours: an end is
  the principal's, and how much is `[limits]`.
- **Not who it's given to.** The service that holds the principal holds
  which policy it has.

The cases every check is held to are in `tests/specs/policy/`
of this workspace, each a policy and the answers it gives.

<!-- spec-reference: policy -->
