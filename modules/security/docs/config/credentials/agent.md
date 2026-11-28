# Agent Credentials

What proves a call comes from an agent, and what keeps another
program from acting as it: its key, its memory's embedding proof and
its sandbox proxy's secret. Inspect the agent that holds these credentials.

```sh
lnk agent show
```

## An agent's key

The key the harness is handed at each start, its endpoint's key, which
proves the agent's calls to its bridges without ever being sent.

- **Proves:** a call to a bridge comes from that agent: an HMAC-SHA256
  with the key of a challenge the bridge sends on that connection.
- **Made by:** the harness, at each start of the agent: its endpoint's
  key.
- **Held by:** the harness, and Link outside the sandbox, in the
  machine's keyring (`~/.lnk/safe/agents/credentials`, 700), which every
  sandbox hides.
- **Travels:** never. A proof is of one challenge, so it gives the key
  to no one, and is of no use on another connection.
- **Lasts:** until the agent stops: [`lnk agent stop`](../../../../agents/agent/docs/agents/harnesses/use/README.md#one-running-in-a-terminal) and `remove`
  unbind it. On a machine that runs agents only while they answer, a
  key is made for each binding and dropped when it ends. OpenClaw's
  gateway token is the one kept across starts, in the agent's folder.
- **May:** [call its agent's bridges, as that agent alone](../../../../agents/agent/docs/agents/more/security.md).

## A memory embedding proof

What memory checks before it embeds for an agent, made from the agent's
key and sent only on this machine's Unix sockets.

- **Proves:** a memory engine's embedding request belongs to the agent's
  current binding. It is HMAC-SHA256 of `memory embedding binding` and
  that binding's identity, using its agent key.
- **Held by:** memory, as a redacted secret, and its Unix embedding
  endpoint while checking the call. The endpoint receives no agent key.
- **Travels:** only on this machine's Unix sockets, as
  `Authorization: Link-Binding <agent>:<proof>`. HTTP endpoints receive
  no binding proof. The receiver derives the owner from its caller's
  kernel identity and the current binding, never an owner header.
- **Lasts and revoked:** until the binding ends or its key changes.
  A shared memory service proves each agent separately; a slot's user
  may use only its own binding.
- **May:** embed that agent's memory, for that binding's owner alone.

## The sandbox proxy's secret

A random path segment in the URLs a sandboxed program reaches its
proxy with (`/_link/url/<secret>/...`), so another program on the
machine [can't use the proxy as the agent](../../../sandbox/docs/tooling/sandbox/security.md). Made at each start, held by
the proxy and the program it serves, and never leaves the machine.

### How an adapter hands them on

An adapter hands its harness those URLs in files of this user's alone,
never on the command line of the harness's own `config set`: a tool's
address written into the harness's settings file itself, the model's address
in its `.env`, which its settings name by variable (`${VAR}`, which
OpenClaw and Hermes read), and DeepSeek Harness's in Link's patch over
its settings, mode 600.
