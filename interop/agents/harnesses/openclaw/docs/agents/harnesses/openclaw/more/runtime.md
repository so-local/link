# Runtime

The openclaw adapter installs, configures, and runs the native harness for Link. This page describes the process and configuration boundary behind the [agent workflow](../guide.md).

## How it runs

[`lnk agent start`](../../../../../../../../../modules/agents/agent/docs/agents/README.md#create-an-agent) installs the OpenClaw release Link pins, with its own
Node, with its programs in read-only `~/.lnk/exec/harnesses/openclaw`
and its mutable home in `~/.lnk/data/agents/<agent>/harnesses/openclaw`.
It runs its gateway in the sandbox, with `openclaw acp` beside it behind
`lnk-openclaw serve`, its endpoint. [Security](../security.md) lists what it installs and
the ports it uses.

### Its gateway first

When it starts a gateway, `lnk-openclaw serve` waits for its listener
and that boot's database-admission readiness record, within the startup
deadline, before launching `openclaw acp`. Recovery after an unclean boot
can delay admission after the port opens. A gateway already listening
there is reused. Its token, `LNK_OPENCLAW_GATEWAY_TOKEN`, is kept in
OpenClaw's `.env` and read back from there.

### Your connections

Link's bridge carries your connections, Telegram, Slack, Discord and
Signal, over the Agent Client Protocol (ACP) to `openclaw acp`, with Link's key for this start as
its key, each chat a session of its own on its default agent.
OpenClaw holds no bot's credential, and [`lnk agent chat send`](../../../../../../../../../modules/agents/agent/docs/agents/chat/README.md#talk-to-it-here) reaches it.

### Its scheduled jobs

Link's scheduler runs them once you add it with
[`lnk agent tool add scheduler`](../../../../../../../../../modules/agents/agent/docs/agents/README.md#add-tools-and-skills): each reaches OpenClaw as a message from the
schedule, and its answer goes to the chat the schedule names.

### Native Schedule Import

The adapter exports native `cron/jobs.json` through the portable
`schedules` state kind, including named omissions for jobs whose timing,
execution options or destination cannot be represented. Native identity
and explicit timing survive repeated export.

### Revision-checked Commands

`state schedules <native-root>` reads only that source for migration.
`state clear-schedules <revision> <managed-native-root>` archives its exact
exported revision and clears the managed source after its runner stops.
It refuses changes made since export and refuses filesystem links when
clearing. A retry finds the retained archive. Portable import never
writes jobs into this harness's native cron file.

### Its memory and your files

Its memory, `MEMORY.md`, `USER.md` and `memory/`, stays in its
workspace, in its home. Your files are in the agent's `home`.

## Files it keeps

What Link writes in the harness's home for it, beside OpenClaw's own
files, each in `<harness home>`.

### `.openclaw/link-tools.json`

The MCP servers Link added to OpenClaw's settings, one per tool, so it
removes only its own. The file names its schema version.

### `.openclaw/link-providers.json`

The model providers Link set in OpenClaw's settings, so when the model
changes it removes only those, and a configure that changes nothing
runs no removal at all. A provider's address is `LNK_OPENCLAW_MODEL_URL`,
which reaches a model on this machine through Link's proxy with the
proxy's secret in it, so the address is never in the settings file.

### `.lnk/data/harnesses/openclaw/doctor-release`

The release `openclaw doctor --fix` last succeeded for, so it runs once
for each release, and at each start until it succeeds.
