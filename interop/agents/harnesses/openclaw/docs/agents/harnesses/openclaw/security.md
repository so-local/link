# Security

What `lnk-openclaw` installs and runs, beyond [what Link's sandbox
guarantees every harness](../../../../../../../../modules/agents/agent/docs/agents/more/security.md).

## Installing it

What the adapter runs to install OpenClaw, and what checks it.

### Its installer, pinned

It runs OpenClaw's official installer, what `openclaw.ai/install-cli.sh`
serves, pinned: the script comes from OpenClaw's repository at a
release's commit, and runs only if its SHA-256 is the one in this
adapter's source.

### From npm

It installs that release with `--version`, from npm, which checks the
package against the registry's hash. OpenClaw's npm dependencies are
resolved at install time, since the package comes with no lockfile: they
aren't pinned.

### Installation and Runtime Files

The installer runs over HTTPS in its installation sandbox, writing the
permitted program payload and harness home. Programs live under
`~/.lnk/exec/harnesses/openclaw` and are read-only to the running
harness. Mutable configuration and native state stay in its private home.

### Its own Node

It brings its own Node, checked against Node's published checksums,
with no Homebrew, no onboarding and no service of its own.

### Its updates

A new OpenClaw release comes with a release of this adapter, never
fetched on its own, since the adapter pins its version.

## Running it

What it may do while it runs, and where its secrets are.

### It asks for its programs and toolchains

OpenClaw asks, and runs them only once you allow it, for its install
folder and the shell, git, Node, and Python toolchains, which name the
native programs it invokes. Its installed Node and program payload are
read-only at runtime, and a program none of those names stays refused.

### Its file helper on its own fallbacks

`@openclaw/fs-safe`, `FS_SAFE_NATIVE_MODE=off`, in its gateway and in
each `openclaw` command Link runs: natively it opens files with
`openat2`, which Link's sandbox refuses on Linux, as it can't check
that call's mode, and it then fails rather than falling back.

### What keeps it to its home

The sandbox stays what keeps OpenClaw to its home; the file helper's
own guard against paths that leave a folder is its fallbacks', which it
calls best-effort.

### Its state at a switch

[`lnk agent harness set`](../../../../../../../../modules/agents/agent/docs/agents/harnesses/use/README.md#switch-in-one-command)
asks OpenClaw's hub for supported state over the Agent State Protocol
(ASP). Its adapter exposes native transcripts, instructions, skills,
and memory notes. Historical conversation and tool records grant no
Link authority and never execute by being imported. Imported instructions
and skills influence the agent within its existing grants.

## Its gateway and browser

What its gateway takes and holds, and when its browser's ports open.

### Its local gateway

OpenClaw's local gateway, on 127.0.0.1, takes Link's key for each start
as its token, kept in `~/.openclaw/.env` in its home (600), which its
settings name (`${LNK_OPENCLAW_GATEWAY_TOKEN}`). [`lnk secret list`](../../../../../../../../modules/security/secret/contract/docs/config/secrets/more/contract.md#listed-for-lnk-secret-list) lists
the gateway's token in each agent's home.

### Its keys and addresses

The model's key and the model's address (`${LNK_OPENCLAW_MODEL_URL}`,
which holds its proxy's secret for a model on this machine) go in that
`.env` too, and its tools' addresses into its settings file itself (600):
none on the command line of `openclaw config set`.

### No key of its own for a hosted model

Behind its proxy, a hosted model's key is Link's to add. At each start,
an API key or token OpenClaw saved for that model's provider is logged
out of its agents' stores, and once its doctor has taken an older
install's `auth-profiles.json` in, that file and the backups beside it
go. A sign-in (OAuth) stays: it can't be given another way.

#### A migrated copy keeps no key Link took

[`lnk agent migrate`](../../../../../../../../modules/agents/agent/docs/agents/README.md#migrate-your-agent) takes each key it moves into Link's secrets out of
the copy's `auth-profiles.json` files at once, through the adapter's
`clean`, so an agent never started keeps none either. A profile holding
another key, or a sign-in, stays. A key an install had already moved into
its agents' stores stays there until the first start logs it out.

### It doesn't announce itself

Its workspace and its log are in its home. Its mDNS is off and it runs
with `OPENCLAW_DISABLE_BONJOUR=1`, so it doesn't announce itself on
your network.

### The canvas and browser control

The canvas is on the gateway's port, behind its token, and so is
browser control, which has no port of its own. Its extension relay
starts at the first browser call, on the gateway's port +10, and asks
for a token too; behind the proxy it can't take that port, so it
doesn't start.

### The managed browser's debugging port

It asks for no credential, and is on the machine's loopback while the
browser runs. On a Mac the sandbox gives OpenClaw no browser to run (it
looks for Chrome in `/Applications`, which the sandbox closes), so it
opens only without the sandbox: with OpenAI on a Mac, or when you turn
the sandbox off.

### Its browser is off unless you allow it

Without the sandbox, or on Linux with `--network.mode host`. Link tells
`lnk-openclaw` whether it runs behind its proxy; where its browser's
ports would be on the machine's loopback (no sandbox anywhere; on
Linux, `--network.mode host` too), `lnk-openclaw needs` names it,
`browser`, and the start asks in a terminal.

### Its answer

No, `--yes`, or no terminal leaves it off (`browser.enabled: false` in
its settings) and OpenClaw runs without it. A yes in a terminal, or `lnk
agent harness-permission add browser --harness openclaw`, turns it on: then its
debugging port, and its extension relay's, are open to every user and
program of the machine while it runs. In the sandbox on a Mac, and
behind its proxy on Linux, it stays on.

### It holds no connection's token

Link's bridge carries Telegram, Slack, Discord and Signal over the Agent Client Protocol (ACP) to
its endpoint, the port after its gateway's, with Link's key for the
start, its gateway token too.

### With OpenAI on a Mac it asks for no sandbox at all

Codex's runtime runs `ps`, which no sandbox on macOS runs. Allowed, it
reads your whole home folder, and every start warns.

## Its ports

Its gateway, 18789 for `main` (another agent's is in its own block),
its endpoint on the port after it, and the ports OpenClaw derives from
the gateway's: +2 to +4 (browser control on
+2, when it has a port of its own), the managed browsers' debugging
ports (+11 to +110). These, and a local model's, are the localhost
ports the sandbox lets it connect to.

## Outside its home

Unsandboxed, OpenClaw writes `~/.openclaw`, `~/openclaw`,
`~/.local/bin/openclaw`, its own service (the LaunchAgent
`ai.openclaw.gateway`, or the systemd unit `openclaw-gateway.service`),
`/tmp/openclaw` and `/tmp/openclaw-state-locks-<uid>`.

### Finding and removing them

[`lnk agent harness show --harness openclaw`](../../../../../../../../modules/agents/agent/docs/agents/harnesses/use/harness/README.md#inspect-the-installation) looks for these; `lnk agent
harness delete --harness openclaw` deletes them with its home. Its own service
would run outside the sandbox, so [`lnk agent start`](../../../../../../../../modules/agents/agent/docs/agents/README.md#create-an-agent) stops it and keeps
its file in `~/.lnk/safe/agents/stopped-services`: Link runs OpenClaw.
