# What Are Agents

**An agent is an AI that gets things done for you.** A chatbot answers, but
an agent also acts: it searches, reads, writes, and keeps going until the job
is finished. With the
[scheduler](../../../tools/scheduler-tool/docs/agents/tools/scheduler/README.md)
tool, an agent can also start work at a time you choose and follow up with
you. Every agent is made of the same five parts, so once you know the parts
you know every agent.

## Five Parts Make an Agent

State, harness, model, connections, and tools make up every agent.

### [State](state/README.md)

**An agent's state is what makes the agent yours.** State is the files and
folders the agent has access to, and the ones that make its tools work: its
instructions, its skills, its memory, and the folders you give it. When you
move or share an agent, its state goes with it.
[State](state/README.md) explains what an agent keeps and where.

### [Harness](../../../harnesses/docs/agents/harnesses/README.md)

**An agent's harness is what makes the agent work.** The harness runs the AI
model, hands it tools, and keeps the conversation going.
[How a Harness Works](#how-a-harness-works) explains what a harness does, and
[Harnesses](../../../harnesses/docs/agents/harnesses/README.md) lists the
harnesses you can choose.

### [Model](../../../model/docs/agents/models/README.md)

**An agent's model is the AI that does the thinking.** A model comes from a
provider like Anthropic or OpenAI, or it runs on your own computer. You can
change an agent's model and keep everything else, as
[Models](../../../model/docs/agents/models/README.md) explains.

### [Connections](../../../connections/docs/agents/connections/README.md)

**An agent's connections are where you reach the agent.** A connection links
the agent to Discord, Telegram, Slack, or Signal, so you can text your agent
from your phone. [Connections](../../../connections/docs/agents/connections/README.md)
lists every channel.

### [Tools](../../../tools/docs/agents/tools/README.md)

**An agent's tools are what the agent can do.** Memory, web search, file
access, and the scheduler are tools. Each tool gives the agent one more way to
act, and [Tools](../../../tools/docs/agents/tools/README.md) lists them all.

## Two Kinds of State

An agent's state comes in two kinds: files the agent changes, and a Safe the agent only obeys.

### Files the Agent Changes

**An agent reads and writes its own files.** Its instructions, its skills, its
memory, and the folders you give it are files the agent can change as it
works.

### [Safe](../../../../storage/safe/docs/config/safe/README.md) the Agent Can't Touch

**An agent's Safe holds what steers the agent.** The
[Safe](../../../../storage/safe/docs/config/safe/README.md) keeps the agent's
settings, its sandbox rules, its secrets, and its credentials. The agent works
under what the Safe says, but the agent can never read or change the Safe. So
nothing an agent does can change what the agent is allowed to do.

## How a Harness Works

A harness turns a model that answers once into an agent that works until the job is done.

### Think, Act, and Look

**An agent thinks, acts, and looks before it answers.** Ask your agent which of
your files mention a deadline. The agent doesn't answer at once. It searches
your files and reads what it finds. Then it notices one more folder, searches
that too, and only then tells you.

### Loop

**Each round of thinking, acting, and looking is one turn of a loop.** The
model decides what to do next. The harness does it and shows the model what
happened. The loop repeats until the model has an answer or needs to ask you
something.

### Everything Around the Loop

**A harness is that loop and everything around it.** The harness keeps your
conversations, loads the agent's instructions and skills, and runs its tools.
A good harness makes the loop fast, safe, and pleasant to work with.

## Why the Parts Matter

The parts tell you what changes when you move or migrate an agent.

### [Moving](move/README.md) Moves Every Part

**Moving an agent moves all five parts.** Its state, harness, model,
connections, and tools go to the cloud together, and they all come back with
you. [Move Your Agent](move/README.md) shows how.

### [Migrating](migrate/README.md) Swaps One Part

**Migrating swaps one part.** When you come from OpenClaw, Hermes, or
DeepSeek, Local Link replaces only the harness. Your agent keeps everything
else, as [Migrate to Local Link](migrate/README.md) shows.

## [Teams](team/README.md) of Agents

**A team lets agents talk to one another.** Put three agents on a team, and
each one gets tools to ask the others for help. A team is a group of agents,
not part of any one agent. [Create an Agent Team](team/README.md) shows how.

## [Sandbox](../../../../security/sandbox/docs/tooling/sandbox/README.md) Keeps Every Agent Safe

**Every agent Local Link runs works inside a sandbox.** The sandbox lets an
agent reach only the folders and hosts you allow, and it asks you before
anything more. The sandbox isn't part of the agent, and the sandbox is what
makes running an agent safe.
[Sandbox](../../../../security/sandbox/docs/tooling/sandbox/README.md) explains
how the sandbox works.
