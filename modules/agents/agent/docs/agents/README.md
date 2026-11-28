# Agents

> Run Your Agent Here or in Your Cloud

**An agent is an AI that gets things done for you.** Every agent is made of the
same five parts: its state, its harness, its model, its connections, and its
tools. [What Are Agents](agent.md) explains each part and how a harness works.
Start one blank or from an archive someone shared, once
[Local Link](../../../../../README.md#install) is installed and you have a
hosted model's API key or choose a local model when asked.

<!-- quickstart -->

## Start Your Agent

Your first start creates Onboarding, an agent that answers what Local
Link can do, and asks for its model. The agent keeps running in the
background. [Start Your Agent](start/README.md#start-your-agent-in-the-background) explains
the first-start choices and how to start another agent.

```sh
lnk agent start
```

## Talk to Your Agent

Open a conversation in this terminal and send your first message. You can
also name a conversation, follow up on it, or send a single message from a
script with [Terminal Chat](chat/README.md#talk-to-it-here).
Connect a messaging app to text your agent from your phone.
[Talk to Your Agent](chat/README.md) covers the terminal, scripts, and every
messaging app.

```sh
lnk agent chat
lnk agent connect telegram|slack|discord|signal
```

## Create a Shareable Agent

Share an agent you made with your friends and your community, and start the
agents they share, each as one archive file. Local Link's
[Ready-Made Agents](../../../../../agents/docs/agents/ready-made/README.md), a programmer, a researcher, a
coach, and a personal assistant, are where you start: import one and
make it yours. [Create a Shareable Agent](share/README.md) covers making an agent worth
sharing, exporting it with
[`lnk archive export`](../../../../tooling/archive/docs/tooling/archive/README.md#export),
and importing someone else's.

```sh
curl -fLO https://github.com/so-local/link-releases/releases/latest/download/link-coach.tar.gz
lnk archive import link-coach.tar.gz
lnk agent start --agent coach
```

## Leverage Link Harness

Link Harness is Local Link's own harness. You edit its instructions and skills
as files, inspect the prompt it sends, and keep your conversations in an event
log you control. [Leverage Link Harness](../../../harnesses/link/docs/agents/link.md)
shows how to get the most from it, and the
[controlled benchmarks](../../../harnesses/link/docs/agents/harnesses/link/benchmarks.md)
compare its startup and idle memory.

```sh
lnk agent harness set link
```

## Create an Agent Team

Give several agents distinct purposes and a shared folder, then address the
team or one member. [Create an Agent Team](team/README.md) covers creating the members,
choosing a lead, and keeping their work together when they move. A
[Team Specification](../config/specs/team.md) describes a team as a TOML file.

```sh
lnk agent team list
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

## Migrate to Local Link

Import an agent you already run with OpenClaw, Hermes, or DeepSeek.
[Migrate to Local Link](migrate/README.md) explains the plan you
review, the state that moves, and how to resume or undo a migration.

```sh
lnk agent migrate
```

## Parts of an Agent

Each part of an agent has its own section, which says what the part is and
every command it has.
[Ready-Made Agents](../../../../../agents/docs/agents/ready-made/README.md) are the six agents Local Link made
to start from, [State](state/README.md) is what your agent knows and keeps,
the [Connections](../../../connections/docs/agents/connections/README.md)
catalogue lists every messaging app, and [More](more/README.md) covers the
agent's settings, access, security, and runtime interfaces.

### Harnesses

Switch the program that runs your agent. The agent keeps its model, files, and
connections. [Switch Harnesses](harnesses/use/README.md) explains what happens
to each harness's saved state, and the
[Harnesses](../../../harnesses/docs/agents/harnesses/README.md) catalogue
lists them all.

```sh
lnk agent harness set mini|link|openclaw|hermes|deepseek
```

### Models

Switch your agent to an available local model or to a hosted provider's model.
[Switch Models](models/agent/README.md) shows how to install a local model, add
provider credentials, and see what each harness supports. [Models](../../../model/docs/agents/models/README.md)
covers the local runtimes and hosted providers that supply the models.

```sh
lnk agent model set anthropic/claude-sonnet-5-5
lnk agent model set openai/gpt-6.1-sol
lnk agent model set local/qwen3:8b
```

### Tools

Choose what your agent can do beyond talking:
[Web](../../../tools/web-tool/docs/agents/tools/web/README.md),
[Browser](../../../tools/browser-tool/docs/agents/tools/browser/README.md),
[Desktop](../../../tools/desktop-tool/docs/agents/tools/desktop/README.md),
[Run](../../../tools/run-tool/docs/agents/tools/run/README.md),
[Code](../../../tools/code-tool/docs/agents/tools/code/README.md),
[File](../../../tools/file-tool/docs/agents/tools/file/README.md),
[Memory](../../../tools/memory-tool/docs/agents/tools/memory/README.md), and
[Scheduler](../../../tools/scheduler-tool/docs/agents/tools/scheduler/README.md).
[Tools and Skills](tools/use/README.md) explains each tool's permissions and
how to add skills, and the [Tools](../../../tools/docs/agents/tools/README.md)
catalogue lists every tool.

```sh
lnk agent tool list
lnk agent tool add web
lnk agent tool remove web
```

<!-- docs-tree:start -->

## Docs

### Pages

- [What Are Agents](agent.md): An agent is an AI that gets things done for you
- [Start Your Agent](start/README.md): Your first `lnk agent start` creates Onboarding, an agent that answers what Local Link can do
- [Talk to Your Agent](chat/README.md): Talk to your running agent in a terminal, send a message from a script, or text your agent from your phone
- [Create a Shareable Agent](share/README.md): People share agents the way they share open-source code: start from one someone made, make it your own, and pass it on to your friends or publish it for everyone
- [Create an Agent Team](team/README.md): A team splits a job by purpose
- [Move Your Agent](move/README.md): Move your agent to a box in your own cloud, and back
- [Migrate to Local Link](migrate/README.md): Bring an agent you already run with OpenClaw, Hermes, or DeepSeek under Local Link
- [State](state/README.md): An agent's state is the files and folders the agent has access to, and the ones that make its tools work
- [Switch Harnesses](harnesses/use/README.md): Switch the harness that runs your agent with `lnk agent harness set <harness>`
- [Switch Models](models/agent/README.md): Change what your agent thinks with, in one command
- [Connect Messaging Apps](connections/connect/README.md): Connect your agent to a messaging app so you can text the agent from your phone or workspace
- [Tools and Skills](tools/use/README.md): Choose the tools your agent can call, and add reusable instructions for recurring work

### More

- [More](more/README.md): Inspect your agent's configuration and stored files, understand its security boundaries, and integrate with its runtime interfaces

<!-- docs-tree:end -->
