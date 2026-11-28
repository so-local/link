# Security

What `lnk-hermes` installs and runs, beyond [what Link's sandbox
guarantees every harness](../../../../../../../../modules/agents/agent/docs/agents/more/security.md).

## Installing it

What the adapter runs to install Hermes, and what checks it.

### Its installer, pinned

It runs Hermes's official installer, what
`hermes-agent.nousresearch.com/install.sh` serves, pinned: the script
comes from Hermes's repository at a release's commit, and runs only if
its SHA-256 is the one in this adapter's source.

### A git checkout

It installs that release with `--commit`, a git checkout, so the
commit identifies its upstream base. During install, before the payload
is sealed and published, the adapter applies the guarded native
substitutions described in [Decisions](decisions.md). They keep session catalogues non-blocking, hosted
metadata on inference routes and the private gateway owned through shutdown.
Unexpected source refuses the substitutions and publishes nothing.

### Staged Programs

Installation runs in a sandbox with an explicit staging directory for
programs. The installer receives that directory as its dependency home
and puts its code there with `--dir`, including for root. Link publishes
the complete payload under its protected programs folder; running
harnesses can read that tree but cannot write it.

### Its own Python and Node

Its installer downloads a pinned uv from GitHub and checks its SHA-256,
then starts Hermes's package manager on a Python of the same minor
version: this machine's, or one uv downloads. That package manager
installs its own Python, Node and other tools into the payload's `tools`,
each from its `pm/lock.json` and checked by hash. It runs no setup
wizard and installs no service of its own.

#### Installer's Python

The install step may run programs only from the staged payload, so the
adapter points uv's Python folder (`UV_PYTHON_INSTALL_DIR`) at the
payload's `bootstrap-python`. Left to uv's default, that Python would
land in the agent's home, `~/.local/share/uv`, where the sandbox refuses
to run it. The folder's name is in the adapter's `programs.toml`. Only
the installer runs that Python, and it is published read-only with the
rest of the payload. Hermes's package manager runs uv with its own
interpreter and with Python downloads off, so it never puts a Python in
the agent's home.

### Its Python packages

Hermes's package manager installs them from its `uv.lock`, pinned by
hash, into an environment under the staged payload's `installs` folder.
A sealed manifest selects that environment by a relative path, so moving
the complete payload preserves the exact installed selection.

#### Package Manager Runtime

The already installed package-manager generation is moved into the
payload's native `pm-runtime` location before publication. Its marker
names a contained store interpreter and dependencies by relative paths.
Runtime, session operations and install cache hits require a complete
manifest and package-manager marker; a launcher alone cannot bypass
installation or fall back to mutable dependency state.

### Packages on first use

The adapter installs its supported Anthropic extra during staging,
before any agent chooses a provider. Link's bridge carries connections,
so their native libraries are unnecessary. Configuration writes data and
never installs packages. Extras come from Hermes's `uv.lock`, pinned by
hash, and changing the agent's provider uses the same published payload.

### Sealed Dependency Selection

The adapter checks the exact hashes of the pinned package-manager
sources before applying its relocation workaround. The manifest takes
precedence over mutable dependency-home settings and tools-store
overrides. Sealed activation avoids generation locks and leases;
package-manager writes target the protected payload rather than falling
back to executable tools in data. Unexpected source or an escaping
dependency selection refuses publication.

### Its updates

A new Hermes release comes with a release of this adapter, never
fetched on its own, since the adapter pins its commit.

## Running it

What it may do while it runs, and where its secrets are.

### It asks for its programs and toolchains

Hermes asks, with your permission, to run its own install folder and the
shell, git, Node, and Python toolchains, because its tools run commands.
Its own Python and dependencies live in the read-only program payload,
and a command its tools request runs only if one of those names it. Runtime invocations disable lazy installation and discard
ambient tools-store and install-stamp overrides.

### Its local API

Hermes's local API, on 127.0.0.1, takes Link's key for each start,
which only Link's bridge holds. It holds no connection's token: Link's
bridge carries them.

### Its keys and addresses

Keys go in `~/.hermes/.env` in its home (600), never on a command line.
The model's address goes there too (`LNK_HERMES_MODEL_URL`, which holds
its proxy's secret for a model on this machine), named in its `config.yaml` by
variable (`${LNK_HERMES_MODEL_URL}`, which Hermes reads), and its tools'
addresses into `config.yaml` itself (600): none on the command line of
`hermes config set`.

### Its API's key in `lnk secret list`

[`lnk secret list`](../../../../../../../../modules/security/secret/contract/docs/config/secrets/more/contract.md#listed-for-lnk-secret-list) lists its API's key, the one Link hands it at each
start, in each agent's home. That home is a folder the agent can read,
so the agent can read its own API's key there.

### Not listed: Hermes's own logins

`~/.hermes/auth.json`, which `hermes login` writes when you run it in
the agent's home yourself. Link never runs it nor relies on it: Link
hands Hermes its model's key, or a placeholder for its proxy, in
`.env`. One you made stays in the home, and moves with the agent by [`lnk agent move`](../../../../../../../../modules/agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud).

### Its terminal's home

Its terminal's tools get a home of their own (`terminal.home_mode
profile`) rather than reaching for your `~/.ssh` and `~/.gitconfig`.

## Its state at a switch

[`lnk agent harness set`](../../../../../../../../modules/agents/agent/docs/agents/harnesses/use/README.md#switch-in-one-command)
asks Hermes's hub for state over the Agent State Protocol (ASP).
The adapter reads and writes supported sessions through `SessionDB`,
and maps instructions, skills, and memory notes to native files.
Imported historical records establish no Link authority and do not
replay tool effects. Instructions and skills remain subject to the
agent's existing permissions.

## Its ports

Its endpoint, 8642 for `main` (another agent's is in its own block):
with a local model's, the only localhost ports the sandbox lets it
connect to.

## Outside its home

Unsandboxed, Hermes writes `~/.hermes`, `~/.local/bin/hermes` and its
own service (the LaunchAgent `ai.hermes.gateway`, or the systemd unit
`hermes-gateway.service`).

### Finding and removing them

[`lnk agent harness show --harness hermes`](../../../../../../../../modules/agents/agent/docs/agents/harnesses/use/harness/README.md#inspect-the-installation) looks for these; `lnk agent
harness delete --harness hermes` deletes them with its home. Its own service
would run outside the sandbox, so [`lnk agent start`](../../../../../../../../modules/agents/agent/docs/agents/README.md#start-your-agent) stops it and moves
it aside.

### What it leaves

Every Python Link-managed Hermes installs, the one uv downloads to start
its installer included, is in its protected program payload.
The agent's conversations, memory and configuration remain
data in its Hermes home and travel through the agent's transfer path.

## Portable session archive

`.hermes/link-imports.json` holds exact imported Agent Client Protocol (ACP) updates and their native
projection, mode 600. A native continuation exports the original updates
plus later work. Divergent history preserves both branches. Historical tool
calls become records without effects; thinking and unsupported metadata
remain archival and are named as omissions from live context.
