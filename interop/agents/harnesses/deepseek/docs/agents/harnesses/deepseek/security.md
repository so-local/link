# Security

What `lnk-deepseek` installs and runs, beyond [what Link's sandbox
guarantees every harness](../../../../../../../../modules/agents/agent/docs/agents/more/security.md).

## Installing it

What the adapter downloads, from where, and what checks it.

### Node and `dsh`

DeepSeek Harness has no installer script: this adapter downloads Node
from nodejs.org, checked against the SHA-256 in its source, and
installs `@deepseek-ai/dsh` from npm at a pinned version. npm checks
that package against the registry's hash.

### Its dependencies

Its dependencies are resolved at install time, since it comes with no
lockfile, and some run install scripts (node-pty's, which only sets a
file executable).

### The CAs npm trusts

npm trusts the machine's CAs beside Node's own: those
`NODE_EXTRA_CA_CERTS` names, else the system's bundle
(`/etc/ssl/certs/ca-certificates.crt` or its distribution's), so an
install behind a proxy that re-signs TLS reaches the registry.

### Its updates

A new `dsh` release comes with a release of this adapter, never
fetched on its own, since the adapter pins its version.

## Running it

What it may do while it runs, and where its secrets are.

### It asks for its programs and toolchains

`dsh` runs the Node runtime it installed and the commands its tools request. The adapter therefore asks, subject to your approval, to run its install folder and the shell, git, Node, and Python toolchains, each naming the programs it lets run.

### Its own sandbox and approvals are off

Link's sandbox confines it instead, with the same rights as any
harness: its home and the files folder, the network through its proxy.

### Nothing goes to DeepSeek

Link's settings layer turns off the session log and module list `dsh`
attaches to requests to DeepSeek's API, its telemetry (also
`DSH_TELEMETRY_DISABLED`), and its web search, which uses DeepSeek's
API.

### Its endpoint

It answers only on loopback, and only a request with the key Link hands
it at each start (`Authorization: Bearer`).

### It proves it's the endpoint

Before Link's connections send it that key or a message, it answers
their random challenge with an HMAC of it made with the key, never the
key itself (`proof` in its contract). A program that took its port
while it started or restarted can't, and gets neither.

### The model's key

It's in `dsh`'s store of keys, `~/.deepseek/home/.credentials.yaml`
(600), a placeholder when it's on the wire, which it is by default. The
store wins over a `.env` in the files folder, which `dsh` also reads
keys from.

### Its tools' and a local model's addresses

They hold its proxy's secret. Tool addresses stay in
`~/.deepseek/link-tools.json` (600) and reach `dsh` only as each session's
MCP servers. A local model's address stays in `link.patch.yml` (600);
`dsh` is given that file's path, never the secret on its command line.

### The endpoint's key in `lnk secret list`

[`lnk secret list`](../../../../../../../../modules/security/secret/contract/docs/config/secrets/more/contract.md#listed-for-lnk-secret-list) lists its copy of the endpoint's key
(`~/.deepseek/link.env`) in each agent's home.

### Known gap: other keys in `.env`

Another key in that `.env`, such as `DEEPSEEK_API_KEY`, reaches `dsh`
too. Link's settings send nothing that uses one.
