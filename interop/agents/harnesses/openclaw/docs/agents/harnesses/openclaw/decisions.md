# Decisions

Why `lnk-openclaw` runs OpenClaw the way it does. Link's own decisions
about harnesses are on its [Agents
Decisions](../../../../../../../../modules/agents/agent/docs/agents/more/decisions.md)
page.

## Why carry its state over the Agent State Protocol?

OpenClaw keeps native sessions behind its gateway. Its adapter reads
the native transcript store, and the shared hub retains complete
portable history. The gateway cannot reconstruct foreign native
sessions, so imported history remains in the archive and supplies bounded
context on the next turn. The import names this limit and preserves
existing native bindings for a return.

## Where is OpenClaw's workspace?

In its home (`~/.lnk/data/agents/main/harnesses/openclaw/.openclaw/workspace`),
with its personality and memory, and `~/Link/Agents/main/home` as the
shared folder. Hermes works in `home`.

## How does OpenClaw use a local model?

Only through Ollama, registered with its address and Ollama's own API,
and a context window of 32,768 tokens on both sides: OpenClaw's budget
and Ollama's `num_ctx`. Ollama's default window, a few thousand tokens,
is smaller than OpenClaw's own instructions.

## How do your connections reach OpenClaw?

Over the [Agent Client Protocol (ACP)](https://agentclientprotocol.com),
as they reach every harness: Link's bridge, then the [harness hub](../../../../../../../../modules/agents/harnesses/portability/hub/docs/agents/harnesses/more/protocol/hub.md)
in [adapter endpoint](more/runtime.md), then `openclaw acp` over stdio, which forwards
each prompt to its gateway. OpenClaw runs none of its channels, so one
bot's credential has one reader, a switch or a move never has two
harnesses polling it, and its Slack and Discord, whose sockets couldn't
use Link's proxy, no longer ask for the network as it is.

### Why set Node's warning option before ACP starts?

The pinned OpenClaw starts another Node process to add
`--disable-warning=ExperimentalWarning` for ACP. Its parent stays resident
while the real ACP client runs detached. The adapter supplies that same
option in `NODE_OPTIONS` at the initial launch, preserving the caller's
other options and CA configuration. Native runtime and TLS startup checks
still run. Hub also owns the ordinary ACP process group so wrapper
descendants in that group stop before launcher resources are released.

### How a chat is kept

Each chat is an ACP session of `openclaw acp`, a gateway session behind
it, which the hub keeps by the chat's thread and takes up again after a
restart. The hub's key is Link's key for the start, the gateway's token
too, which only the bridge holds; the gateway's own chat completions
stay off.

## Who runs its scheduled jobs?

Link's scheduler, as for every harness whose contract says `from`: a
schedule's message reaches OpenClaw over ACP, said as the schedule's,
and its answer goes where the schedule says. OpenClaw's own cron is off
(`cron.enabled: false`), as its channels are, so no job fires twice.
Its cron tool is denied too (`tools.deny` gains `cron`, beside any tools
the agent already denies), so the agent sets a schedule with Link's
scheduler, which [`lnk agent tool add scheduler`](../../../../../../../../modules/agents/agent/docs/agents/README.md#tools) turns on, never one on a
cron that never fires.

## What does it need beyond Link's defaults, and when?

No sandbox only with OpenAI on a Mac, where Codex's runtime runs `ps`,
which no macOS sandbox runs. An agent on any connection with Anthropic
can remain sandboxed, with the separately approved any-program grant.

## Why is its browser off without the sandbox, or with `--network.mode host`?

Because there its managed browser's debugging port asks for no
credential on the machine's loopback, where every user and program
reaches it, and so drives the browser as you. Its other browser ports
ask for a token, or don't open in the sandbox.

### Off by default

OpenClaw runs well without its browser, so the safe answer is the
default: `needs` names it as a risk it runs without, `browser`, and only
your yes in a terminal, or `lnk agent harness-permission add browser --harness
openclaw`, turns it on.

### Where it stays on

In the sandbox on a Mac it stays on, since OpenClaw finds no Chrome to
start there; on Linux it stays on behind its proxy, where the port stays
in the sandbox's network.

### How the adapter knows

Link tells the adapter whether it runs behind its proxy (`proxied` in
its settings), so `--network.mode host` is asked about as no sandbox is,
and it takes a Link that doesn't say as no proxy.

## How does OpenClaw get a secret in its settings?

By variable: a value in its settings that holds a secret names a variable
of its `.env` (`${LNK_OPENCLAW_GATEWAY_TOKEN}`,
`${LNK_OPENCLAW_MODEL_URL}`), which OpenClaw reads as it loads its
settings, so `openclaw config set` never has one on its command line,
where every user of the machine sees it.

### The model's address

The model's address is one, since a model on this machine is reached
through its proxy with the proxy's secret in its path. OpenClaw refuses
settings naming a variable that isn't set, so the model's address stays
in `.env` until no provider's entry names it.

### Its tools' addresses

Its tools' addresses, which hold the secret too, are written into its
settings file itself (600), by this adapter rather than `openclaw config
set`.

## Which of OpenClaw's bugs does it work around?

A bug in the OpenClaw release the adapter pins is worked around in the
adapter's `src/workarounds/`, apart from the code meant to stay, each
naming the bug and its report to OpenClaw. A workaround goes once the
pin moves to a release that fixes its bug.

### Its sessions' ids over ACP

The pinned OpenClaw's `openclaw acp` confuses a session's ACP id with its
gateway key, `acp-bridge:<id>`. A `session/resume` after a restart binds
the session to a new, empty one, and `session/list` names each session
by its key. Between the hub and `openclaw acp`, the adapter takes
`resume` out of what it offers, so the hub takes a session up with
`session/load`, which finds it, and gives a list's sessions their ids.
So a chat goes on after OpenClaw restarts, and a switch away carries
each chat as the thread it was.

## Why refresh tool routes separately?

The openclaw adapter follows the [native tool-route refresh](../../../../../../../../modules/agents/harnesses/docs/agents/harnesses/guide.md#refresh-native-tool-routes) policy, keeping changing endpoint addresses separate from structural model and provider configuration.
