# DeepSeek Harness Guide

Run DeepSeek Harness with your chosen provider or local runtime, then connect it to your chats. The adapter supplies its settings, disables DeepSeek telemetry and native web search, and preserves chat bindings across restarts.

```sh
lnk agent start --model.default deepseek/deepseek-v4-pro
lnk agent harness set deepseek
lnk agent connect telegram
```

## Use a model

It uses the model your agent has, hosted with your key or running on
your computer. DeepSeek's own models are on OpenRouter, so `lnk agent
start` with a DeepSeek model asks for your OpenRouter key, or reads it
from `OPENROUTER_API_KEY` when that is set in its environment.

## Talk to it

It speaks no connection itself, so Link carries Telegram, Slack, Discord
and Signal to it, as it does for Link Harness. Each chat is a `dsh`
session of its own that goes on where it was after a restart.

```sh
lnk agent connect telegram
```

## What Link does

Link gives deepseek its own runtime settings and carries each chat through the adapter endpoint. The [runtime reference](more/runtime.md) explains installation, native configuration, and the process lifecycle.

### Existing Installations

[`lnk agent migrate --harness deepseek`](../../../../../../../../modules/agents/agent/docs/agents/README.md#migrate-your-agent)
can copy a standalone installation into Link's managed home. Starting
fresh uses [agent creation](../../../../../../../../modules/agents/agent/docs/agents/start/create.md)
with `--harness deepseek`. Both routes keep DeepSeek as the harness,
which is separate from transferring its history into another runtime.

## Ports

Its endpoint defaults to port 18820; `LNK_PORT` selects the base port
Link assigns to a managed agent. With a local model's, that's the only
localhost port the sandbox lets it connect to.

## Keep Context Through a Switch

DeepSeek exports its native session JSONL files, including zstd-compressed
logs and older generations. Current-format messages and tool records have
readable projections. Older formats and unknown records remain complete
archival data, with omissions named explicitly.

### Imported Conversations

DeepSeek cannot reconstruct imported native sessions. The hub retains the
complete portable history and supplies bounded context with the next turn.
It preserves existing bindings for a return. Required intent, corrections
and memory that exceed the context budget refuse continuation while keeping
the archive. Historical tools never run because they appear in that history.

### Instructions and Skills

The agent's persona, owner and standing instructions reach DeepSeek in
Link's block of its `AGENTS.md`, and the agent's skills are among the
folders it scans. A switch away carries the rest of that `AGENTS.md` and
the skills DeepSeek made; a switch to it writes them there. It keeps no
memory of its own, so Link Memory reaches it as a tool. [Compare Harnesses](../../../../../docs/agents/harnesses/compare.md)
distinguishes native features from portable state mappings.

### Machine Moves

[`lnk agent move`](../../../../../../../../modules/agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud)
uses the adapter's native-state inventory to carry DeepSeek's managed
home and the agent's shared files. Resuming those native files on the
same harness does not require native import of foreign conversations.
The [Agent Protocol](../../../../../../../../modules/agents/harnesses/portability/contract/docs/agents/harnesses/more/agent-protocol.md)
explains the two state paths.

## Inspect Native State

The [runtime reference](more/runtime.md#files-it-keeps) identifies the
native settings, credential store and chat bindings kept in the harness's
private home.

## More about it

Read [Decisions](decisions.md) for the deepseek adapter’s design, [Security](security.md) for its execution and credential limits, and [Troubleshooting](troubleshooting.md) for recovery. Compare its model and state support with [other harnesses](../../../../../docs/agents/harnesses/compare.md).

## Work on the Adapter

[Developing](more/developing.md) gives the deepseek adapter’s local build checks and license. [Troubleshooting](troubleshooting.md) helps recover its running process.
