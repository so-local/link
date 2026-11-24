# Start Your Agent

> Start Small, Keep Your Choices

Start an agent on this computer, choose its model, and reuse its saved
settings on later starts.

```sh
lnk agent start
```

When your agent doesn't answer, or answers slowly, [its log](log/README.md)
says why.

## Check What Starting an Agent Needs

You need a Mac, or a Linux x86-64 computer with bubblewrap installed.
[`lnk up`](../../../../../../docs/more/modules/install.md#pick-your-modules) adds the modules, and also adds the one for a
model on your computer. Local inference uses that computer's resources.
A hosted model needs a key from Anthropic, OpenAI, or OpenRouter, and the
provider bills you.

### Add Cloud Services for Your Agent

[Cloud](../../../../../../docs/services/README.md) connects your own clouds,
runs agents on boxes, backs up files to buckets, and provides networking.
IAM (identity and access management) controls access to resources. Start on
this computer, then add the services your agent needs.

## Start Your Agent in the Background

Your agent keeps running in the background, also after a restart. Each
harness has commands of its own, `lnk agent harness ...`, for your agent's
harness or the one `--harness` names. The names are `link`, `mini`,
`openclaw`, `hermes`, `deepseek`, or one you added. A
hosted model's key is asked for, or read from its variable, such as
`ANTHROPIC_API_KEY`, when that is set where you start the agent.

```sh
lnk agent start
lnk agent start --model.default local/qwen3
```

### Answer the First-Start Questions

The first start installs the harness. Then it asks which model your agent
thinks with, which folder the agent works in, and which of Local Link's tools
the agent gets. Memory is picked unless you say otherwise. Web asks for a
Brave Search or Tavily key for its search. Browser needs Chromium or Google
Chrome on this computer. Desktop needs Linux's `Xvfb`. Later starts reuse what
you chose.

### Ask Your Agent About Local Link

Add [Link MCP](../../../../tools/mcp/docs/agents/tools/mcp/README.md) with `lnk agent tool add mcp`, and you can ask your agent what it can do. The tool reads the offline manual and inspects enabled capabilities. Declared changes ask through the sandbox before they run. Link MCP is off by default, and the personal ready-made agent comes with it on.

### Add the Tools That Start Off

A new agent runs [Link's
specification](../../../contract/spec/docs/config/specs/guides/features.md#new-agents):
file is on, so when you ask, the agent changes its files, instructions, and
skills. Scheduler, run, code, and Link MCP are off until you add each one with
`lnk agent tool add <name>` or enable it in your specification. Add reusable task
instructions through [Skills](../tools/use/skill/README.md).

### Choose Tools Once per Agent

The first start asks about tools once per agent, whatever you answer. Run
`lnk agent tool add <tool>` to add one later. Picking Memory offers an
embedding model too. Without the model, memory searches by words only.

### Start an Agent Without a Terminal

Without a terminal to ask in, the agent's folder is `~/Link/Agents/<name>/home`.
`--run.files <dir>` picks another. The start says so and names the command for
the agent's tools, and a later start in a terminal asks.

### Choose From the Models Offered

The first start offers a model already running on this computer. With none,
an Apple Silicon Mac is offered one to install, a few GB in size. After that
comes an API key from Anthropic, OpenAI, or OpenRouter.

### Read What Every Start Shows

Every start ends with the table `lnk agent show` shows. The table says
whether your agent runs, its model, its tools, where you text it, and where
its files are. Running means the harness answers. A key the provider refuses
shows at your first message.

### Follow the Suggested Next Commands

The start suggests `lnk agent chat` first, which talks to your agent in this
terminal. Then comes `lnk agent connect telegram` while no connection reaches
your agent. Next comes `lnk agent tool add run`, which lets your agent run
programs, until it has Run. Until your agent has Link MCP, `lnk agent tool add
mcp` lets your agent read Link's docs and run Link's commands. Last come
`lnk agent harness set <harness>` for another harness and
`lnk agent model set <model>` for another model.

```sh
lnk agent chat
```

### Give a Hosted Model's Key

Give a hosted model's key when `lnk` asks for it, or through the provider's
environment variable: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, or
`OPENROUTER_API_KEY`. Never give the key as a command-line argument, which
other users of this machine can see. `lnk agent start --help` and
`lnk agent model set --help` say the same. To change the model later, see
[Switch Models](../models/agent/README.md). Your agent answers only while its
machine is on.

## Start From an Agent Someone Made

Skip the setup and start from an agent someone already made, one of Local
Link's four ready-made agents or one your friends or community shared.
[Share Agents](../share/README.md) says how an agent travels as one archive
file, and [Ready-Made Agents](../ready-made/README.md) introduces the four and
how far each one goes.

## Manage the Agent

Give each agent its own name and settings with [Create Another Agent](create.md). Inspect an agent's configuration with [Show an Agent](show.md), and diagnose runtime state with [Status and Logs](status.md). Control the agent's service with [Stop and Restart](stop.md), and remove the agent with [Delete Your Agent](delete.md). [Security](security.md) and [Decisions](decisions.md) explain the lifecycle boundaries.

<!-- docs-tree:start -->

## Docs

### Pages

- [Create Another Agent](create.md): Create an additional agent with its own name, model, files and connections
- [Check Your Agent](status.md): Ask whether your agent runs, and follow what it does
- [Inspect Your Agent](show.md): See everything about your agent in one readout, or print the prompt its model gets
- [Stop and Restart](stop.md): Stop your agent and everything started for it, until it starts again
- [Delete Your Agent](delete.md): Delete an agent and choose whether to keep its files for recovery
- [Agent Log](log/README.md): Your agent's log is what its harness and Local Link wrote while it ran

### More

- [Decisions](decisions.md): Why agents keep stable identities, separate folders and predictable startup settings
- [Security](security.md): How shared bridges and idle-agent processes retain each agent's identity and isolation

<!-- docs-tree:end -->
