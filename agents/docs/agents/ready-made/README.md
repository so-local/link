# Ready-Made Agents

> Start an Agent Today, Then Make It Yours

Anyone can make an agent, and anyone can share one. Local Link's ready-made agents are where you start: import one, make it yours, and share what you make.

## Choose an Agent

[Onboarding](onboarding.md) answers what Local Link can do, from its manual,
and your first [`lnk agent start`](../../../../modules/agents/agent/docs/agents/README.md#start-your-agent) creates Onboarding for you.
[Programmer](programmer.md) works in one of your repositories and commits its own
reviewed changes. [Researcher](researcher.md) searches the web and
writes reports that cite every claim. [Coach](coach.md) keeps a training
and food plan with you and checks in on it each evening.
[Personal Assistant](personal.md) learns about you, keeps your lists and
reminders, and plans your day.

| Agent | Name | Tools | Programs |
|---|---|---|---|
| Onboarding | `onboarding` | file, scheduler, and Link MCP | none |
| Programmer | `programmer` | run, file, scheduler, code, and Link MCP | its toolchains, Git included |
| Researcher | `researcher` | file, scheduler, and web | none |
| Coach | `coach` | file and scheduler | none |
| Personal Assistant | `personal` | file, scheduler, memory, web, and browser | none |

## Bring One In

Each agent is released with Local Link as one file, `link-<name>.tar.gz`,
on the [latest release](https://github.com/so-local/link-releases/releases/latest).
Download the one you want and import it from the folder you saved it
in. Import checks the whole
archive, asks whether to keep the agent's name, and places a new agent
here with an ID of its own. A name already taken is never overwritten,
and `--select` renames an agent without a terminal.

```sh
lnk archive import link-coach.tar.gz --select agent.coach=agent.my-coach
```

## Start and Chat

[`lnk agent start`](../../../../modules/agents/agent/docs/agents/README.md#start-your-agent) starts an imported agent in the background, and the
first start asks for your model provider's credential, as for any agent.
[`lnk agent chat`](../../../../modules/agents/agent/docs/agents/chat/README.md#manage-history) then opens a conversation in this terminal. A messaging
app is connected on each machine, since its bot token never travels in
an archive, as [Connect Messaging Apps](../../../../modules/agents/agent/docs/agents/connections/connect/README.md)
shows.

```sh
lnk agent start --agent programmer
lnk agent chat --agent programmer
```

## Models by Rule

Each agent's model follows Local Link's rule: the current Sonnet for
coding, and the current Haiku for everything else. Programmer's settings set
`claude-sonnet-5-5` on Anthropic, and the other three keep Link's
default, `claude-haiku-5-5` on Anthropic or `gpt-6-luna` on OpenAI.
[Switch Models](../../../../modules/agents/agent/docs/agents/models/agent/README.md) covers every provider and
models on your computer.

```sh
lnk agent model set local/qwen3:8b --agent coach
```

## What Each Can Reach

Every ready-made agent runs in the sandbox its archive's sandbox file
sets. Each writes only its files folder, `~/Link/Agents/<name>/home`
unless you name another, and its own instructions and skills. Each
reaches any public host through its proxy, since its model is hosted.
Only Programmer and Hacker run programs, and Hacker alone reads your
whole home and your network's machines, to test the sandbox's walls. Import names what a
sandbox file opens beyond Link's own before you start the agent.

```sh
lnk agent show --agent programmer --permissions
```

### Narrow or Widen the Sandbox

[`lnk agent permission remove`](../../../../modules/agents/agent/docs/tooling/sandbox/permissions/README.md#choose-what-your-agent-can-reach) takes a grant back and
[`lnk agent permission add`](../../../../modules/agents/agent/docs/tooling/sandbox/permissions/README.md#choose-what-your-agent-can-reach) gives one, with the sandbox specification's
field flags. Coach needs only its model, so its hosts can shrink to
Anthropic's API, as
[Choose What the Sandbox Can Reach](../../../../modules/security/sandbox/docs/tooling/sandbox/reach.md)
explains for every grant.

```sh
lnk agent permission remove --agent coach --internet.hosts '*'
lnk agent permission add --agent coach --internet.hosts api.anthropic.com
```

## Make an Agent Your Own

**Every agent is plain files you can change.** Instructions say how an
agent works, skills teach it a task, tools give it reach, and its
sandbox file says where it may go. Change one file and the agent
changes with it, so Coach learns your sport and Programmer learns your
team's review habits. The possibilities are endless.
[Make an Agent Worth Sharing](../../../../modules/agents/agent/docs/agents/share/worth-sharing.md)
shows each change. Change one file of your agent today.

## Share What You Make

**An agent you made is one file away from anyone who wants one.**
[`lnk archive export`](../../../../modules/tooling/archive/docs/tooling/archive/README.md#export)
writes your agent as an archive, with its instructions, skills, tools,
and sandbox file, and never its secrets. Anyone imports that archive as
you imported this one, so a friend starts your agent in one command.
[Create a Shareable Agent](../../../../modules/agents/agent/docs/agents/share/README.md)
walks through the whole path. Share the agent you made.

```sh
lnk archive export --select agent.coach
```

## How Each Agent Is Kept

An agent's folder in `agents/ready-made/` holds `archive.toml` and the
folders its `[map]` lays out: `Agents/<name>/` for its instructions,
skills, and `home`, and `Safe/Agents/<name>/` for its `agent.toml` and
`sandbox.toml`. Each setting carries a comment saying why it differs
from Link's defaults.

### README Beside the Archive

Each folder also holds a `README.md` beside `archive.toml`, showing the
agent's sandbox file and the commands to start it. An archive may carry
a README at its top, as the
[Archive Package](../../../../packages/archive/docs/tooling/archive/package.md)
says. The README travels in the released `.tar.gz`, and import leaves it
out, so the agent's files stay exactly what the `[map]` names.

### Blank Until You Speak

Each agent's Home starts with only headings, table headers, and empty
fields, so nothing in it is an example the agent could take for yours.
Reference data a skill reads, such as the coach's nutrition values, sits
beside that skill instead, and every agent saves a suggestion only once
you say yes. A
[repository check](../../../../tests/repository/docs/more/developing/testing/repository-checks.md#ready-made-agents)
refuses a row, list item, or filled field in any Home.

### Packed and Imported on Every Release

The release packs each folder with
[`lnk archive export --from`](../../../../modules/tooling/archive/docs/tooling/archive/README.md#export),
which packs a folder from a checkout the same way, and Link's tests
import the same folders. `agents/scripts/pack.sh` packs every agent into
`dist/link-<name>.tar.gz` and imports each into your own `~/Link`.

## Contribute Here

**We've only scratched the surface of what these agents can do.** We
haven't spent much time tuning them yet, so a world of better skills,
sharper instructions, and smarter sandboxes is still open. Every skill,
instruction, and sandbox rule you send makes an agent better for
everyone who imports it. Each agent's page names a good
first change, and [How Each Agent Is Kept](#how-each-agent-is-kept)
shows the files a change touches and how every release packs and
imports them. Send us the change that made your copy better.

<!-- docs-tree:start -->

## Docs

- [Onboarding](onboarding.md): Onboarding is your first agent: ask what Local Link can do, and Onboarding answers from Local Link's manual with the command to type
- [Programmer](programmer.md): Programmer is a ready-made coding agent that explores, plans, tests, reviews, and commits its own changes in one repository
- [Researcher](researcher.md): Researcher is a ready-made research agent that searches the web, reads its sources, and writes a cited report into its files folder
- [Coach](coach.md): Coach is a ready-made personal coach for running, cycling, swimming, lifting, team sports, and everyday eating
- [Personal Assistant](personal.md): Personal Assistant is a ready-made agent, named `personal`, that learns about you, keeps your lists and reminders, and plans your day

<!-- docs-tree:end -->
