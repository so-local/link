# Runtime

The hermes adapter installs, configures, and runs the native harness for Link. This page describes the process and configuration boundary behind the [agent workflow](../guide.md).

## How it runs

[`lnk agent start`](../../../../../../../../../modules/agents/agent/docs/agents/README.md#start-your-agent) installs the Hermes release Link pins, with its own
Python and Node, in the read-only `~/.lnk/exec/harnesses/hermes`, keeps its
configuration and conversations in
`~/.lnk/data/agents/<agent>/harnesses/hermes`, and runs the published
`hermes-acp` there, in the sandbox, behind `lnk-hermes serve`, its endpoint.

### Native Gateway

Link also owns the private native gateway in the foreground, and
the frontend attaches to it. These are separate processes: the gateway runs
the agent and discovers its tools, and Link closes both during a stop.

### Managed Gateway

The gateway Link starts runs with `LNK_HERMES_MANAGED_GATEWAY` set to
1, so its workarounds know Link manages it.

### Each message

Link's bridge sends it each message over the Agent Client Protocol (ACP), with Link's key for this
start, each chat a Hermes session of its own. Its terminal starts in
your files folder.
[Security](../security.md) lists what it installs and the ports it uses.

### Its scheduled jobs

Link's scheduler runs them once you add it with
[`lnk agent tool add scheduler`](../../../../../../../../../modules/agents/agent/docs/agents/README.md#tools): each reaches Hermes as a message from the
schedule, and its answer goes to the chat the schedule names.

## Native Schedules

Hermes’s native `cron/jobs.json` exports as portable `schedules`, preserving native identity and explicit timing across exports. Jobs that cannot be represented retain named omissions for their timing, execution options, or destination.

### Revision-checked Commands

Hermes’s `state schedules <native-root>` inspects the selected native source. After the runner stops, `state clear-schedules <revision> <managed-native-root>` archives that exported revision and clears only the unchanged managed source, refusing filesystem links. Repeating the clear finds the retained archive; portable import does not populate native cron.

## Files it keeps

What Link writes in the harness's home for it, beside Hermes's own
files.

### `.hermes/link-tools.json`

The MCP servers Link added to Hermes's settings, one per tool, in
`<harness home>`, so it removes only its own. The file names its schema
version.

### `.hermes/link.env`

The key of its endpoint for this start, `ENDPOINT_KEY`, mode 600, which
only Link's bridge sends: kept beside Hermes's `.env`, never in it, so
Hermes never reads it. The model's address is in Hermes's own `.env`,
as `LNK_HERMES_MODEL_URL`, which `config.yaml` names, so the address is
never on a command line.

### `.hermes/link-threads.json`

Which Hermes session is each chat's thread, so a chat goes on where it
was after a restart.
