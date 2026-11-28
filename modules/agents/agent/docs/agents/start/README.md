# Start Your Agent

> Start Small, Keep Your Choices

Your first `lnk agent start` creates [Onboarding](../../../../../../agents/docs/agents/ready-made/onboarding.md), an agent that
answers what Local Link can do. Start more agents, blank or from an
archive someone shared, and each keeps its settings for later starts.

```sh
lnk agent start
```

When your agent doesn't answer, or answers slowly, [its log](log/README.md)
says why.

## Check What Starting an Agent Needs

You need a Mac, or a Linux computer on x86_64 or arm64 with bubblewrap
installed.
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
thinks with. An agent you create blank also asks which folder it works
in and which of Local Link's tools it gets, since an archive's agent
comes with its own. Memory is picked unless you say otherwise. Web asks for a
Brave Search or Tavily key for its search. Browser needs Chromium or Google
Chrome on this computer. Desktop needs Linux's `Xvfb`. Later starts reuse what
you chose.

### Ask Onboarding About Local Link

On a computer with no agent, `lnk agent start` creates Onboarding from
the archive the agent module carries, so it needs no download. Ask
Onboarding "What can you do?" or "How do I start another agent?", and
it answers from Local Link's offline manual with the command to type.
Onboarding reads the manual through [Link MCP](../../../../tools/mcp/docs/agents/tools/mcp/README.md),
which any agent gets with `lnk agent tool add mcp`.

### Start Several Agents

Once you have several agents, `--agent <name>` starts one, and `--all`
starts every one. A start that names neither lists your agents and asks
which.

```sh
lnk agent start --all
```

### Add the Tools That Start Off

A new agent runs [Link's
specification](../../../contract/spec/docs/config/specs/guides/features.md#new-agents):
every tool is off, including file, scheduler, run, code, and Link MCP,
until you add each one with `lnk agent tool add <name>` or
enable it in your specification. Add reusable task
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

## Stop and Restart

Stop your agent and everything started for it, until it starts again.

```sh
lnk agent stop
lnk agent restart
```

Normal `lnk agent start` also registers automatic startup. On macOS,
launchd keeps it running and starts it at your next login after reboot.
On native Linux it enables the user service. `lnk agent stop` removes
that automatic startup as well as stopping it now. `restart` applies
stop and start to an existing agent; it keeps its saved model, files
and tool configuration. `--agent <name>` selects another agent.

### Stop an Agent in a Terminal

One started with `--foreground` is `running in a terminal` to
`status`, and stops with Ctrl-C there, `lnk agent stop`, or a SIGTERM or
SIGHUP to `lnk`, which it passes on to the harness and waits for (one
asleep for being idle just ends). On Linux, `lnk` killed outright
(SIGKILL) sends its harness a SIGTERM all the same, so nothing it
started keeps the agent's ports. Until then another `lnk agent start`
says it runs there. Its output shows there and goes to its log too, as
a service's does, so `lnk agent log show` reads it either way.

### How Long a Stop Waits

`lnk agent stop` waits up to 35 seconds for an agent in a terminal to
end: the 30 its harness has to finish the turns it's answering, and a
moment to exit. Only then does it say the agent didn't stop.

### What a Stop Ends

A stop ends everything started for the agent, however it runs (in a
terminal, as a background service, or woken when a message comes):
its harness, its bridge and connections, its scheduler's runner and the
calendar's wake, and its background commands. Nothing of it fires a
job or answers a message until it starts again.

## Start From an Archive

Skip the setup and start from an agent someone already made, as
[Create a Shareable Agent](../share/README.md#start-from-a-community-agent) shows.

## Manage Your Agents

[Create and Delete Agents](create.md) gives each agent its own name and
settings, and removes one you no longer need.
[List and Show Agents](show.md) finds your agents, says whether each runs, and
shows everything about one. [Security](security.md) and
[Decisions](decisions.md) explain the lifecycle boundaries.

<!-- docs-tree:start -->

## Docs

### Pages

- [Create and Delete Agents](create.md): Create an additional agent with its own name, model, files and connections, and delete an agent when you no longer need it
- [List and Show Agents](show.md): List your agents, ask whether one runs, and see everything about it in one readout
- [Agent Log](log/README.md): Your agent's log is what its harness and Local Link wrote while it ran

### More

- [Decisions](decisions.md): Why agents keep stable identities, separate folders and predictable startup settings
- [Security](security.md): How shared bridges and idle-agent processes retain each agent's identity and isolation

<!-- docs-tree:end -->
