# Access

Each agent acts as a service of its own, which is who its secrets are given to and who its teams name.

## An Agent's Service

A service is a program acting on its own behalf, as opposed to you
acting yourself. An agent runs with no one at the keyboard, so Link
gives every agent a service to act as. When a secret is given to an
agent, or a team names who is on it, it names that service.

### Its own ID

The service is `service:<id>`, an ID of 12 letters and digits that Link
mints with the agent. It is never the agent's own ID, `agent:<id>`: the
agent's ID names what runs, for logs and measures, and the service's
names who acts, for grants. An ID is never given again once its agent is
deleted, so a grant to a deleted agent's service never reaches an agent
made later with its name.

### In Its Identity

[This machine's records](settings/files.md#this-machines-records) name it, beside the agent's ID, in
`~/.lnk/safe/agents/main/identity.toml`, never in the agent's
`agent.toml`, so an agent carried in an archive arrives without one and
is given its own.

### When it's made

The service is minted at the agent's first save: `lnk agent create`,
`main`'s first `lnk agent start`, a team definition that makes a member,
or an agent brought in from another harness. This computer keeps every
service ID it minted or took in, and each one retired, in
[`~/.lnk/safe/agents/services.toml`](settings/files.md), beside the agents' own IDs in
`ids.toml`.

### Moves and deletes

The service goes wherever the agent goes. A move carries it, and the
machine it arrives on takes it in, refusing one it retired or another
agent there has. Bringing the agent back from its buckets, with the
internal `lnk agent restore`, takes it back with the agent. `lnk
agent delete` retires it with the agent's ID, and takes back every
secret given to it.

## What the Service Holds

A service holds what is given to it, and nothing else.

### Secrets

Your secrets are given to an agent's service: those you add with `lnk
secret add <name> --to agent:<agent>`, and the agent's own model provider credentials and
connection tokens that `lnk agent start` and `lnk agent connect` keep for
it. Each grant names the service, the action, `secret.secret.pass` for
a sealed secret or `secret.secret.read` for a plain one, and the [secret](../../../../../security/secret/docs/config/secrets/README.md#who-a-grant-names).

### Bridge calls

Each call an agent's harness makes on a bridge proves the agent's ID and
its service with the key it was given at start, and the bridge reads the
agent's roles, a team's among them, by the service. A harness is told
its service in `LNK_AGENT_SERVICE`, beside `LNK_AGENT_ID`.

### Teams

A team's file lists its members by their agent IDs, and each member's
service is read from its settings when the team is read: a teammate's
message is taken as from its service. Another account's agent is named
in the file with its service, as that account gives it.

<!-- docs-tree:start -->

## Docs

- [An Agent's Service](#an-agents-service): A service is a program acting on its own behalf, as opposed to you acting yourself
- [What the Service Holds](#what-the-service-holds): A service holds what is given to it, and nothing else

<!-- docs-tree:end -->
