# Interface

What other programs can rely on from `lnk cloud`, and where the
commands a cloud adapter, `lnk-<kind>`, answers are. An adapter whose
account hosts agents answers the agent module's commands too, its own
contract's.

## `lnk cloud`

The two commands other programs read, each printing JSON and never an
account's keys.

### `lnk cloud list --json`

`[{"name", "kind", "place", "identity", "login"}]`, one entry per
connected account. Read by [`lnk agent move`](../../../../../../agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud) (whether where it goes is a
cloud account) and [`lnk agent list`](../../../../../../agents/agent/docs/agents/start/create.md#run-more-than-one-agent) (the accounts to ask whether they
host agents).

### `lnk cloud env <name> --json`

One account as `link_cloud_contract::Account`: `{"name", "kind",
"place", "identity", "login", "region"?, "profile"?, "project"?,
"settings"?, "env"}`. Exits nonzero when there's no such account.

### Its `settings` and `env`

`settings` holds ordinary selectors and credential-file paths, and
`env` guarded credential values. The adapter combines them only for an
explicitly authorized cloud CLI child.

### Who reads it

[`lnk box`](../../../../../../compute/box/docs/services/boxes/README.md#boxes), [`lnk storage`](../../../../../../storage/storage/docs/services/storage/README.md#storage), and `lnk agent move` and `list` (an account
that hosts agents), for every adapter call, which each then makes
itself (`link_cloud_contract::call`); and `lnk storage create
--cloud` and `connect --cloud`.

## Cloud adapters (`lnk-<kind>`)

What `lnk-aws` and `lnk-gcp` answer, each command reading a request on
stdin and printing a reply, is the contract crate's:
[Cloud Adapters](../../../../contract/docs/services/cloud/more/adapters.md).

### Accounts from an existing sign-in

An installed module [may answer `cloud.accounts`](../../../../../../../core/framework/docs/more/modules/protocol.md#questions), the typed
`link_cloud_contract::ACCOUNTS` question, through its program's
`lnk-<module> answer cloud.accounts`.
It reads `{}` and returns `{"accounts": [...]}` in `Account`'s shape,
each in its envelope. `list` and `env` include
these accounts without saving them. A discovered name may match a saved
account only when its adapter kind and identity match; a conflict fails.
