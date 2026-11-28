# Bridges

> Carry Caller Identity Across the Boundary

What every bridge out of an agent's sandbox speaks, through
`link-sandbox-bridge`.

## Bridges (`link-sandbox-bridge`)

The contract is
[`link-sandbox-bridge`](https://github.com/so-local/link/blob/main/modules/security/sandbox/bridge/src/lib.rs),
re-exported by [`link-sandbox-binding`](../../../../binding/docs/tooling/sandbox/more/bridges/binding.md) with this machine's keyring beside it. A bridge is a way out of an agent's
sandbox beside its network: the way in, a linked connection's
`send.sock`, the store bridge and the [files bridge](../../../../../../../agents/stores/contract/docs/agents/more/stores/files-bridge.md), and [each of its
proxy's routes](../../../../../contract/client/docs/tooling/sandbox/more/contract/client.md).
Each takes calls on a Unix socket, one call to a connection.

### 1. The challenge

The bridge writes `{"protocol", "challenge"}` as a line of JSON
(`link_sandbox_bridge::Challenge`): the bridge protocol's version it speaks,
and 64 random hex characters.

### The protocol's version

The bridge protocol is declared in `link-sandbox-bridge`'s `protocol.toml`: the
version this release speaks and [the oldest it still accepts](../../../../../../../../packages/version/docs/more/libraries/types/version.md#protocol-versions), read into a
`link_version::Protocol`.
Each side states its version first: the bridge in its challenge, the
caller in who it is. A caller refuses a bridge older than the oldest it
accepts, before it proves anything, and a bridge refuses such a caller
before it checks the proof, naming the protocol and [`lnk upgrade`](../../../../../../../../docs/more/modules/guide.md#upgrade).

### 2. Who calls

The caller writes who it is, a line of at most 4 KiB,
`link_sandbox_bridge::Proven`: `{"protocol", "identity": {"agent", "service", "owner"},
"proof"}`, its protocol's version, the
agent's ID, service ID and owner (`link_sandbox_bridge::Identity`, each nonempty
value 1 to 64 letters,
digits, `.`, `_`, `-` and `@`), within 5 seconds of the challenge
(`link_sandbox_bridge::PROOF_WAIT`), the whole exchange. An omitted or
empty service acts as none and cannot form a policy principal.

#### Its proof

HMAC-SHA256, keyed with the agent's key, of `link bridge proof`, a zero
byte, the challenge, a zero byte, the agent's ID, a zero byte, its
service ID, a zero byte and its owner, in hex
(`link_sandbox_bridge::proof`).

#### Its action

Then its action, the bridge's own, as a line of its own, of at most what
the bridge takes (`link_sandbox_bridge::CALL_MAX`, 48 MiB, at the most: a
store's event twice over and a files bridge's largest object as
base64).

### 3. The answer

The bridge looks the agent's key up (`link_sandbox_bridge::Keys`) and refuses a
call whose agent isn't bound or whose proof isn't that agent's before
it reads the action; it takes the caller as its ID, service and owner,
never teams it names. It answers with a line, its own or the error
envelope, then closes the connection.

#### The error envelope

`link_sandbox_bridge::Failure`, `{"error": "<why>"}`, with `"why"` when a caller
acts on it: `refused` (nothing done: who's calling isn't proved, or what
it sent isn't a call the bridge reads), `busy` (nothing done: call
again), `conflict` (nothing done: what the call expected isn't so any
more). A bridge's own answers never hold `error`.

## A stream call

A stream call (`link_sandbox_bridge::stream`, served by `serve_streams`) is for
a bridge whose answer is bytes as they come, a route's: steps 1 and 2
as above, then the caller sends what the call carries (a request's
body) as it comes, without waiting. The bridge answers one line, its
verdict (its own, or the error envelope), then its answer as it comes,
writing nothing before the verdict. Either side ends it by closing.

## What it reaches of another's

A proved call reaches only its own agent's things, unless a role lets
it reach another's: a teammate's way in, or a team's folder. Whether
it may is `link_policy::allows`'s to say, never the bridge's
(IAM).
The bridge gives `link_sandbox_bridge::grants` the proved identity and the
roles it reads at that very call, such as each team's role, and asks
for the action on the resource: `agent.message` on `agent:<id>`, or
`bucket.write` on `bucket:team-<id>/drafts/`.

## The server

Every bridge runs on one server (`link_sandbox_bridge::serve`, or `serve_with`
for one whose answers are async).

### Before the proof

Connections not yet proved hold places of their own, 4 of each peer's,
the user at the other end (`link_sandbox_bridge::PEER_PROVING`), a peer's next
closing its own oldest, answered `busy`, with room for 16 peers
(`link_sandbox_bridge::PEERS`, 64 in all, `PROVING`), past which a new peer's is
answered `busy`.

### A proved call

A proved call holds one of the bridge's places, one of its agent's and
one of its owner's, and is answered `busy` past any. Its action arrives
within the bridge's wait, and its answer is taken within another
(`link_sandbox_bridge::Limits`, each bridge's own). `take_async` reads a call's
action within 30 seconds (`link_sandbox_bridge::CALL_WAIT`).

### A caller's side

`link_sandbox_bridge::call` and `call_at` are a caller's side; `take` and
`take_async`, one call taken by a program that takes its calls itself,
with the same deadline before the proof.

### The test every bridge passes

Every bridge's tests run `link_sandbox_bridge::conformance::check` (the
`conformance` feature) against it, listening: a wrong key, a proof
replayed on a new connection, a malformed or overlong `Proven`, an
action it can't read, that none of those runs, a caller that never
answers or answers a byte at a time, an action sent a byte at a time, a
peer's flood, one agent's calls past its places and everyone's past the
bridge's, an answer never taken, and the envelope.
