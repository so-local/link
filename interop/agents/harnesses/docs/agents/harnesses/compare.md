# Compare Harnesses

Compare the harnesses Link can create and control, the existing installations it can adopt, and the state each adapter can transfer. Starting fresh and bringing an agent you already use are separate choices.

## Start Fresh

All five selections below can run a new agent through Link, with its model configuration, files, tools, and messaging connections. [Harness Adapters](adapters/README.md) introduces the three third-party adapters. [Link Harness](../../../../../../modules/agents/harnesses/link/docs/agents/link/README.md) and [Mini](../../../../../../modules/agents/harnesses/mini/docs/agents/harnesses/more/mini/README.md) are Link's own implementations.

| Harness | Hosted model routes | Local model interface |
|---|---|---|
| Link Harness | Anthropic, OpenAI, OpenRouter | Any runtime Link supports |
| Mini | Anthropic, OpenAI, OpenRouter | Any runtime Link supports |
| Hermes | Anthropic, OpenAI, OpenRouter | Any runtime Link supports |
| OpenClaw | Anthropic, OpenAI, OpenRouter | Ollama |
| DeepSeek | Anthropic, OpenAI, OpenRouter | Any runtime Link supports |

### Create an Agent

Create a separate agent when you want to try a harness without switching an existing one. [Create Another Agent](../../../../../../modules/agents/agent/docs/agents/start/create.md) explains agent selection and isolation. Provider credentials and native model requirements remain those of the chosen integration.

```sh
lnk agent create work --harness hermes
```

## Bring an Existing Installation

[Migrate Harnesses](../../../../../../modules/agents/agent/docs/agents/harnesses/use/migrate/README.md) copies a supported installation into an agent-owned home and runs the same harness under Link. Migration is separate from selecting a harness for a fresh agent, and it does not convert every native resource into another harness's format.

| Harness | Standalone installation migration |
|---|---|
| Link Harness | Not a migration source |
| Mini | Not a migration source |
| Hermes | Supported, including profiles |
| OpenClaw | Supported, including agents |
| DeepSeek | Supported |

## Switch Conversations

[Switch Harnesses](../../../../../../modules/agents/agent/docs/agents/harnesses/use/README.md) retains the agent's shared configuration and files while selecting another harness. The [Agent Protocol](../../../../../../modules/agents/harnesses/portability/contract/docs/agents/harnesses/more/agent-protocol.md) explains how portable export and import differ from native restoration. The source must export what the destination can use, and each switch names unsupported state.

| Destination | How imported conversations continue |
|---|---|
| Link Harness | Supported content becomes native history |
| Mini | User and assistant text becomes native history |
| Hermes | Supported content becomes native sessions |
| OpenClaw | Retained archive and bounded context |
| DeepSeek | Retained archive and bounded context |

### Native History

Link Harness and Hermes retain exact imported updates separately from the native records they can use. Text, completed tool calls, and supported media can become native history. Thinking and other unsupported content remain archival, with omissions named. Mini's portable import and export carry user and assistant text, while its own saved conversations retain richer tool and provider state.

### Bounded Continuation

OpenClaw and DeepSeek retain complete portable history beside their native sessions and supply selected historical context with the next prompt. They preserve native bindings for a return rather than reconstructing foreign native sessions. Required intent, corrections, and memory must fit the context budget or continuation is refused with the archive intact. Historical tool calls never execute during import.

## Carry Other State

Portable state support is declared kind by kind. The [state protocol](../../../../../../modules/agents/harnesses/portability/agent-state-protocol/docs/agents/harnesses/more/agent-state-protocol.md) defines instructions, skills, memory, tool descriptors, session settings, and schedules, but each integration chooses the native resources it can expose or restore.

| Harness | Instructions, skills, and native memory |
|---|---|
| Link Harness | Shared agent configuration and Link Memory |
| Mini | Home instruction file, no native memory mapping |
| Hermes | Adapter exports and imports supported resources |
| OpenClaw | Adapter exports and imports supported resources |
| DeepSeek | Instructions and skills both ways, no native memory |

### Tools and Settings

Link supplies the tools declared in the agent's specification, including memory, through the adapter. Hermes can export and import its own MCP server definitions. OpenClaw exports its own definitions but does not import them. The switch names harness-local servers that stay behind. Defining a server in one harness's settings does not add it to the agent's shared specification, and session settings support does not imply copying all native configuration.

### Schedules

Link Scheduler can deliver scheduled messages through the common agent interface. Hermes and OpenClaw additionally export native jobs for adoption into Link's schedule definitions. That conversion is one way, preserving timing and identity while giving the managed job one firing owner. The other adapters do not supply this native-job conversion.

## Move Between Machines

[Move Your Agent](../../../../../../modules/agents/agent/docs/agents/move/README.md) transfers workspace files, shared collections, and each adapter's declared native state, then installs and configures the harness on the destination. All five have this native-data transfer path on supported machines. It is independent of whether the destination can import another harness's conversations.

## Choose for Your Work

Start with the native behavior you want and the state you need to bring. The individual guides give runtime requirements, model configuration, sandbox exceptions, and recovery behavior. These choices concern Link's current adapters, rather than every capability the upstream products offer.

### Link Harness

Choose [Link Harness](../../../../../../modules/agents/harnesses/link/docs/agents/link/README.md) for Link's default personal agent, with its shared configuration, skills, memory, and conversation management. Its agent runtime and protocol implementation are Link's own, and it installs no third-party harness runtime.

### Mini

Choose [Mini](../../../../../../modules/agents/harnesses/mini/docs/agents/harnesses/more/mini/README.md) to inspect or embed a small text-and-tools loop. It saves complete native messages, but has no skill loader, media input, automatic compaction, or model fallback. A growing conversation can exceed its model's context window.

### Hermes

Choose [Hermes](../../../hermes/docs/agents/harnesses/hermes/guide.md) for Nous Research's agent, its native skills and memory, and imported conversations written into its session database. It supports the local runtimes Link discovers, with its documented context requirement. Link also manages its private gateway and protocol frontend.

### OpenClaw

Choose [OpenClaw](../../../openclaw/docs/agents/harnesses/openclaw/guide.md) for its personal-assistant behavior and personality files, including an installation you already use. Foreign conversations continue through bounded context. Local models use Ollama, and OpenAI on macOS requires the sandbox exception described in its guide.

### DeepSeek Harness

Choose [DeepSeek](../../../deepseek/docs/agents/harnesses/deepseek/guide.md) for its native coding tools and persistent sessions under Link's management. Its portable mapping centers on conversation history and bounded continuation. Link disables DeepSeek-specific telemetry and hosted features its own provider route does not need.

## Extend the Choice

The [adapter overview](adapters/README.md) explains contributor and harness-creator routes. Shared work focuses first on personal agents, with Hermes followed by OpenClaw as the main third-party targets. [Integration Decisions](adapters/decisions.md) explains that focus and why native formats and compatibility work stay in adapters.
