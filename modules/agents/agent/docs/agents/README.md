# Agents

> Run Your Agent Here or in Your Cloud

Make any agent you can imagine, run it on this computer or in your own cloud,
and [share it](share/README.md) with your friends and your community. Start
one from scratch, bring in one you already run, or start from one someone
shared, once [Local Link](../../../../../README.md#install) is installed and
you have a hosted model's API key or choose a local model when asked.

<!-- quickstart -->

## Create an Agent

Start your agent and choose its model when asked. The agent keeps running in
the background. [Start Your Agent](start/README.md#start-your-agent-in-the-background) explains
the first-start choices and how to start another agent. An
[Agent Specification](../config/specs/agent.md) describes your agent as a TOML file,
and [More](more/README.md) covers its configuration, access, security, and
runtime interfaces.

```sh
lnk agent start
```

## Start Chatting

Open a conversation in this terminal and send your first message. You can
also name a conversation, follow up on it, or send a single message from a
script with [Terminal Chat](chat/README.md#talk-to-it-here).

```sh
lnk agent chat
```

## Share Agents

Share an agent you made with your friends and your community, and start the
agents they share, each as one archive file. Local Link provides four
[Ready-Made Agents](ready-made/README.md) to start from, a programmer, a
researcher, a coach, and a personal assistant, and says plainly how far each
one goes. [Share Agents](share/README.md) covers making an agent worth
sharing, exporting it with
[`lnk archive export`](../../../../tooling/archive/docs/tooling/archive/README.md#export),
and importing someone else's.

```sh
curl -fLO https://github.com/so-local/link-releases/releases/latest/download/link-coach.tar.gz
lnk archive import link-coach.tar.gz
lnk agent start --agent coach
```

## Migrate Your Agent

Import an agent you already run with OpenClaw, Hermes, or DeepSeek.
[Migrate Harnesses](harnesses/use/migrate/README.md) explains the plan you
review, the state that moves, and how to resume or undo a migration.

```sh
lnk agent migrate
```

## Connect Your Channels

Connect a messaging app to text your running agent from your phone.
[Connect Messaging Apps](connections/connect/README.md) shows how to create a
bot, supply its credentials, and confirm the bot is yours. Each app has its own
page:
[Telegram](../../../../../interop/agents/connections/telegram/docs/agents/connections/telegram/README.md),
[Slack](../../../../../interop/agents/connections/slack/docs/agents/connections/slack/README.md),
[Discord](../../../../../interop/agents/connections/discord/docs/agents/connections/discord/README.md), and
[Signal](../../../../../interop/agents/connections/signal/docs/agents/connections/signal/README.md).
The [Connections](../../../connections/docs/agents/connections/README.md)
catalogue lists them all.

```sh
lnk agent connect telegram|slack|discord|signal
```

## Change Your Harness

Switch the program that runs your agent. The agent keeps its model, files, and
connections. [Switch Harnesses](harnesses/use/README.md) explains the
supported harnesses and what happens to their saved state. Each harness has its
own page:
[Mini](../../../harnesses/mini/docs/agents/harnesses/more/mini/README.md),
[Link Harness](../../../harnesses/link/docs/agents/link/README.md),
[OpenClaw](../../../../../interop/agents/harnesses/openclaw/docs/agents/harnesses/openclaw/README.md),
[Hermes](../../../../../interop/agents/harnesses/hermes/docs/agents/harnesses/hermes/README.md), and
[DeepSeek](../../../../../interop/agents/harnesses/deepseek/docs/agents/harnesses/deepseek/README.md).
The [Harnesses](../../../harnesses/docs/agents/harnesses/README.md) catalogue
lists them all. The [Agent Protocol](../../../harnesses/portability/contract/docs/agents/harnesses/more/agent-protocol.md)
explains the model every harness shares, and [Harness Adapters](../../../../../interop/agents/harnesses/docs/agents/harnesses/adapters/README.md)
separates creating a harness fresh from adopting an existing installation.

```sh
lnk agent harness set mini|link|openclaw|hermes|deepseek
```

## Change Your Model

Switch your agent to an available local model or to a hosted provider's model.
[Switch Models](models/agent/README.md) shows how to install a local model, add
provider credentials, and see what each harness supports. [Models](../../../model/docs/agents/models/README.md)
covers the local runtimes and hosted providers that supply the models.

```sh
lnk agent model set anthropic/claude-sonnet-5-5
lnk agent model set openai/gpt-6.1-sol
lnk agent model set local/qwen3:8b
```

## Move Your Agent

Move your agent to a box in your own cloud account, or bring the agent back to
this computer. [Move Your Agent](move/README.md) covers connecting the cloud,
provider billing, and which files and conversations move with the agent.

```sh
lnk cloud connect aws|gcp
lnk agent move aws|gcp
lnk agent move here
```

## Add Tools and Skills

Choose what your agent can do beyond talking, and add task instructions the
agent can reuse. The tools are
[Web](../../../tools/web-tool/docs/agents/tools/web/README.md) to fetch pages,
[Browser](../../../tools/browser-tool/docs/agents/tools/browser/README.md) to
use a site,
[Desktop](../../../tools/desktop-tool/docs/agents/tools/desktop/README.md) to
use an app, [Run](../../../tools/run-tool/docs/agents/tools/run/README.md) to
run programs, [Code](../../../tools/code-tool/docs/agents/tools/code/README.md)
to read code, [File](../../../tools/file-tool/docs/agents/tools/file/README.md)
to edit files,
[Memory](../../../tools/memory-tool/docs/agents/tools/memory/README.md) to
search what the agent knows, and
[Scheduler](../../../tools/scheduler-tool/docs/agents/tools/scheduler/README.md)
to work later. [Tools and Skills](tools/use/README.md) explains search keys,
each tool's permissions, and how to add skills you can inspect and edit. The
[Tools](../../../tools/docs/agents/tools/README.md) catalogue groups the
providers by the work they do.

```sh
lnk agent tool list
lnk agent tool add web
lnk agent tool remove web
```

## Build a Team

Give several agents distinct purposes and a shared folder, then address the
team or one member. [Teams](team/README.md) covers creating the members,
choosing a lead, and keeping their work together when they move. A
[Team Specification](../config/specs/team.md) describes a team as a TOML file.

```sh
lnk agent team list
```

<!-- docs-tree:start -->

## Docs

### Pages

- [Start Your Agent](start/README.md): Start an agent on this computer, choose its model, and reuse its saved settings on later starts
- [Chat](chat/README.md): Talk to your running agent in a terminal, or send a message from a script
- [Share Agents](share/README.md): Make any agent you can imagine, share it with your friends and then your community, and start the agents they share with you
- [Ready-Made Agents](ready-made/README.md): Local Link provides four ready-made agents, examples you import, start, and build on, and the first agents anyone can share
- [Switch Harnesses](harnesses/use/README.md): Switch the harness that runs your agent with `lnk agent harness set <harness>`
- [Switch Models](models/agent/README.md): Change what your agent thinks with, in one command
- [Connect Messaging Apps](connections/connect/README.md): Connect your agent to a messaging app so you can text the agent from your phone or workspace
- [Move Your Agent](move/README.md): Move your agent to a box in your own cloud, and back
- [Teams](team/README.md): A team splits a job by purpose
- [Tools and Skills](tools/use/README.md): Choose the tools your agent can call, and add reusable instructions for recurring work

### More

- [More](more/README.md): Inspect your agent's configuration and stored files, understand its security boundaries, and integrate with its runtime interfaces

<!-- docs-tree:end -->
