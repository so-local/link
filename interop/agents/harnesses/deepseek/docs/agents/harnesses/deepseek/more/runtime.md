# Runtime

The deepseek adapter installs, configures, and runs the native harness for Link. This page describes the process and configuration boundary behind the [agent workflow](../guide.md).

## What Link does

Link installs it, gives it settings of its own, disables DeepSeek telemetry, module inventory and native web search and runs it behind an endpoint of its own.

### It installs it

Link installs Node from nodejs.org, checked against the SHA-256 in its
source, and then `dsh` from npm at the version it pins. Both go in
`~/.deepseek` in the harness's home.

### It gives it settings

It gives `dsh` a settings layer of its own, `~/.deepseek/link.patch.yml`,
that names your model, sets your files folder as the workspace and adds
its runtime configuration. The hub gives each session your tools and
memory as HTTP MCP servers. The model's key goes in `dsh`'s
own store of keys, as `LNK_MODEL_CREDENTIAL` in its `.env`, never on a command
line.

### DeepSeek Telemetry and Search

Your model's requests go to your provider, and Link turns off the
session log, the module list, the telemetry and the web search that
`dsh` would send to DeepSeek.

### It runs it behind an endpoint

It runs one `dsh` behind an endpoint, defaulting to port 18820 (`LNK_PORT`
overrides it), and starts it again
if it stops. Stopped with a SIGTERM or SIGHUP, the endpoint stops `dsh`
with it and reaps its direct child during shutdown, including a
cancelled start.

### Its scheduled jobs

Link's scheduler runs them once you add it with
[`lnk agent tool add scheduler`](../../../../../../../../../modules/agents/agent/docs/agents/README.md#add-tools-and-skills): each reaches DeepSeek Harness as a message
from the schedule, and its answer goes to the chat the schedule names.

### One message at a time

A chat's messages run one at a time in its Agent Client Protocol (ACP) session. The hub keeps
that session by the chat's thread and takes it up again after a
restart, so each answer stays with the conversation that requested it.

## Files it keeps

What Link writes in the harness's home for it, beside `dsh`'s own files.

### `.deepseek/link.patch.yml` and `link.env`

Link's settings layer over `dsh`'s, and its endpoint's key, both mode
600 in `<harness home>`, written at each start.

### `.deepseek/home/.credentials.yaml`

`dsh`'s store of keys, mode 600 in `<harness home>`, with the model's,
or its placeholder when the key is on the wire.

### `.deepseek/home/AGENTS.md`

`dsh`'s user-level instructions in `<harness home>`, which it reads at
each session's start. Link writes the agent's persona, owner and standing
instructions into a block of its own there at each start; the rest of the
file is the agent's, or what a switch to DeepSeek brought in.

### `.deepseek/threads.json`

Each chat's `dsh` session in `<harness home>`, so a chat goes on where
it was after a restart.
