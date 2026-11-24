# OpenClaw Guide

Run OpenClaw with a hosted model or Ollama, then use its personality and memory from your existing chats. The adapter manages its gateway and supplies Link’s tools while keeping native channels and cron disabled.

```sh
lnk agent start
lnk agent harness set openclaw
lnk agent connect telegram
```

## Run another agent on it

An agent of its own can run on OpenClaw from its start, beside the one
you switched.

```sh
lnk agent create work --harness openclaw
```

## Use a hosted model

Anthropic, OpenAI and OpenRouter work with your own key. Without
`--model.default`, it uses [Link's default for the provider](../../../../../../../../modules/agents/agent/docs/agents/README.md).

## Use a local model

OpenClaw runs local models only through Ollama, with a context window of
32,768 tokens. Ollama's own default, a few thousand tokens, is smaller
than OpenClaw's instructions, so Link sets the larger window on both
sides. For a model in LM Studio, llama.cpp, vLLM or Jan, use
[Hermes](../../../../../hermes/docs/agents/harnesses/hermes/README.md).
Keep Ollama running, as the Ollama app does or `ollama serve` in a
terminal of its own, then start the agent with a local model.

```sh
lnk up
lnk agent start --model.default local/qwen3
```

## Bring an Existing OpenClaw Agent

[`lnk agent migrate --harness openclaw`](../../../../../../../../modules/agents/agent/docs/agents/README.md#migrate-your-agent)
discovers the installation's agents and workspaces, copies the selected
ones, and handles supported secrets, connections, and native jobs.
The original files remain available and its service is kept stopped.
Starting fresh uses the agent creation command above instead.

## Keep context when switching

[`lnk agent harness set`](../../../../../../../../modules/agents/agent/docs/agents/harnesses/use/README.md#switch-in-one-command)
uses the [Agent Protocol](../../../../../../../../modules/agents/harnesses/portability/contract/docs/agents/harnesses/more/agent-protocol.md)
to control the harness and transfer supported state. The Agent State
Protocol (ASP) carries exposed conversations, instructions, skills, and
memory. OpenClaw continues imported conversations through an archive
and bounded context, while workspace files remain in the shared agent Home.

### Its memory, exported

Its workspace's `MEMORY.md` and `USER.md` become sourced notes when
switching away. The destination receives supported memory through its
own adapter or Link's memory mechanisms. [Compare Harnesses](../../../../../docs/agents/harnesses/compare.md#carry-other-state)
shows which adapters map these notes into native files.

### What others remember

At each start, what your agent remembers elsewhere, Link's notes and
what Hermes remembered, is a block of Link's in the workspace's
`MEMORY.md`, each under its source; the rest of the file is OpenClaw's
own, and only that is exported.

### Each chat's thread

Each chat Link's bridge carries is the thread its session key names
(`telegram:42`), on Telegram, Slack, Discord and Signal.

### Who your agent is

Your agent's persona and owner go into a block of the workspace's
`SOUL.md` at each start, and its standing instructions into one of its
`AGENTS.md`, the rest of each file as it was; its skills are one of
OpenClaw's extra skills folders.

### Switching away

What those two files hold besides, unless it's still OpenClaw's
template, and the workspace's skills come into your agent's folder
wherever it has none.

### Its state, over ASP

Asked over ASP, OpenClaw exports its native SQLite transcript events,
including earlier compacted windows and available cold or reset archives.
Raw records and unknown fields remain in the portable archive. Imported
conversations use the hub's bounded continuation because the gateway
cannot reconstruct native session state. The import reports this limit.

#### Other State

OpenClaw gives and takes its `SOUL.md`, `AGENTS.md`, skills and memory.
It also gives the MCP servers you added to its settings, but not those
Link added, so a switch names each one the next harness won't use.

#### Memory Revisions

Corrections and deletions keep their original source and update the
workspace's actual memory files on return. A native edit since export
conflicts and must be reconciled. Foreign-memory blocks are regenerated
from current revisions. Full sourced memory also accompanies the next
turn through bounded context when it fits.

### Machine Moves

[`lnk agent move`](../../../../../../../../modules/agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud)
transfers OpenClaw's declared native state alongside shared agent files.
The destination installs OpenClaw for that machine and resumes the same
harness. Native-data transfer is independent of the gateway's inability
to reconstruct a foreign native session.

### Clean up an old OpenClaw install

An OpenClaw installed before Link, or run outside its sandbox, leaves
files and a service of its own outside its home, which
[Security](security.md#outside-its-home) lists. `show` finds them, and
`delete` deletes them with its home. Then `start` installs a fresh
OpenClaw.

```sh
lnk agent harness show --harness openclaw
lnk agent harness delete --harness openclaw
lnk agent start
```

## How it runs

Link gives openclaw its own runtime settings and carries each chat through the adapter endpoint. The [runtime reference](more/runtime.md) explains installation, native configuration, and the process lifecycle.

## OpenAI on a Mac

OpenClaw requests permission to run any program within its sandbox.
OpenAI on a Mac additionally requires running without the sandbox. There OpenClaw runs Codex's runtime, which runs `ps`, a
setuid program no sandbox on macOS runs, so OpenClaw asks to run
unsandboxed and every start warns while it does. On Linux it runs
sandboxed.

## Files it keeps

The [runtime reference](more/runtime.md#files-it-keeps) describes Link’s
records beside OpenClaw’s native files, including managed tool and model
entries and the completed release migration.

## More about it

Read [Decisions](decisions.md) for the openclaw adapter’s design, [Security](security.md) for its execution and credential limits, and [Troubleshooting](troubleshooting.md) for recovery. Compare its model and state support with [other harnesses](../../../../../docs/agents/harnesses/compare.md).

## Work on the Adapter

[Developing](more/developing.md) gives the openclaw adapter’s local build checks and license. [Troubleshooting](troubleshooting.md) helps recover its running process.
