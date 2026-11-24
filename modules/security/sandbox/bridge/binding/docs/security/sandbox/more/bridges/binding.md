# Agent Binding

`link-sandbox-binding`: the agents bound on this machine and their keys, kept
outside every sandbox, which [every bridge](../../../../../../docs/security/sandbox/more/bridges.md) checks a call's proof against.
Binding writes an agent's key; unbinding revokes it.

## The machine's keyring

The agent's key is the one made at each start, its `endpoint_key`.
[`lnk agent start`](../../../../../../../../../agents/agent/docs/agents/README.md#create-an-agent) binds it in the machine's keyring,
`~/.lnk/safe/agents/credentials/<agent id>.json` (700 folder, 600 files,
`link_sandbox_binding::Keyring`), and [`lnk agent stop`](../../../../../../../../../agents/agent/docs/agents/harnesses/use/README.md#one-running-in-a-terminal) and `delete` unbind it: a
key no longer there proves nothing.

### `Keyring::hold`

`Keyring::hold` binds an identity for runs that need a binding only
while they last. One a start bound is used as it is, and never unbound
by a run. With none, it's bound with a key of the runs' own, and every
run held meanwhile shares it, each keeping a shared lock on its file of
holders, `<ID>.held`. The last to let go unbinds it, unless a bind since
replaced it. A bind removes the holders' file, so the runs leave a
start's binding alone. Each step is under the keyring's lock.

## Unbinding revokes what the binding was given

`unbind` and `unbind_key` take a `Revoker`, which takes back what was
granted for a binding before its key is removed. The revoker interface
returns no result, so the binding layer cannot certify remote revocation;
callers must inspect the owning service's failures. The agent module passes [the secret module's
`RouteRevoker`](../../../../../../../../secret/contract/docs/config/secrets/more/contract.md#bindings), which attempts to revoke the binding's secret routes.

## A passable keyring

Where each agent's helpers run as a user of their own, outside its
sandbox, a machine keeps a passable keyring (`Keyring::passable`): its
folder 711, so no one but its user lists the agents.

### `Keyring::bind_shared`

An agent bound with `Keyring::bind_shared` has its file 640, of one
group, so the helpers in that group read that agent's key and no other.
A bind again sets the group again, and an unbind removes the file,
before the next agent's helpers are given that group.
