# Ready-Made Agents

> Four Agents to Start From and Build On

Local Link provides four ready-made agents, examples you import, start, and build on, and the first agents anyone can share. Each comes with its instructions, skills, tools, and sandbox for one job, and says plainly how far it goes.

## Choose an Agent

[Programmer](programmer.md) works in one of your repositories and commits its own
reviewed changes. [Researcher](researcher.md) searches the web and
writes reports that cite every claim. [Coach](coach.md) keeps a training
and food plan with you and checks in on it each evening.
[Personal Assistant](personal.md) learns about you, keeps your lists and
reminders, and plans your day.

| Agent | Name | Tools | Programs |
|---|---|---|---|
| Programmer | `programmer` | run, file, and code | its toolchains, Git included |
| Researcher | `researcher` | file and web | none |
| Coach | `coach` | file and scheduler | none |
| Personal Assistant | `personal` | file, scheduler, memory, web, run, and browser | `sh` alone |

## How Far Each One Goes

The four are optional examples, made to prove that agents travel well
and to give you somewhere to start. Local Link is not a coding agent
platform, so Programmer works but isn't the coding agent we use ourselves.
Researcher is a starting point that has had little work. We've worked
with Coach and Personal Assistant, and we think they work. Each agent's
own page says more.

## Bring One In

Each agent is released with Local Link as one file, `link-<name>.tar.gz`,
on the [latest release](https://github.com/so-local/link-releases/releases/latest).
Download the one you want and import it. Import checks the whole
archive, asks whether to keep the agent's name, and places a new agent
here with an ID of its own. A name already taken is never overwritten,
and `--select` renames an agent without a terminal.

```sh
lnk archive import ~/Downloads/link-coach.tar.gz --select agent.coach=agent.my-coach
```

## Start and Chat

`lnk agent start` starts an imported agent in the background, and the
first start asks for your model provider's credential, as for any agent.
`lnk agent chat` then opens a conversation in this terminal. A messaging
app is connected on each machine, since its bot token never travels in
an archive, as [Connect Messaging Apps](../connections/connect/README.md)
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
[Switch Models](../models/agent/README.md) covers every provider and
models on your computer.

```sh
lnk agent model set local/qwen3:8b --agent coach
```

## What Each Can Reach

Every ready-made agent runs in the sandbox its archive's sandbox file
sets. All four write only their files folder, `~/Link/Agents/<name>/home`
unless you name another, and their own instructions and skills. All four
reach any public host through their proxy, since their model is hosted,
and only Programmer and Personal Assistant run programs. Import names what a
sandbox file opens beyond Link's own before you start the agent.

```sh
lnk agent show --agent programmer --permissions
```

### Narrow or Widen the Sandbox

`lnk agent permission remove` takes a grant back and
`lnk agent permission add` gives one, with the sandbox specification's
field flags. Coach needs only its model, so its hosts can shrink to
Anthropic's API, as
[Choose What the Sandbox Can Reach](../../../../../security/sandbox/docs/security/sandbox/reach.md)
explains for every grant.

```sh
lnk agent permission remove --agent coach --internet.hosts '*'
lnk agent permission add --agent coach --internet.hosts api.anthropic.com
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
[Archive Package](../../../../../../packages/archive/docs/tooling/archive/package.md)
says. The README travels in the released `.tar.gz`, and import leaves it
out, so the agent's files stay exactly what the `[map]` names.

### Blank Until You Speak

Each agent's Home starts with only headings, table headers, and empty
fields, so nothing in it is an example the agent could take for yours.
Reference data a skill reads, such as the coach's nutrition values, sits
beside that skill instead, and every agent saves a suggestion only once
you say yes. A
[repository check](../../../../../../tests/repository/docs/more/developing/testing/repository-checks.md#ready-made-agents)
refuses a row, list item, or filled field in any Home.

### Packed and Imported on Every Release

The release packs each folder with
[`lnk archive export --from`](../../../../../tooling/archive/docs/tooling/archive/README.md#export),
which packs a folder from a checkout the same way, and Link's tests
import the same folders. `agents/scripts/pack.sh` packs every agent into
`dist/link-<name>.tar.gz` and imports each into your own `~/Link`.

## Contribute Here

The four ready-made agents get better with every skill, instruction,
and sandbox rule that makes them more useful for their job. Each agent's
own page says where it stops, and
[How Each Agent Is Kept](#how-each-agent-is-kept) shows the files a
change touches and how every release packs and imports them.

<!-- docs-tree:start -->

## Docs

- [Programmer](programmer.md): Programmer is a ready-made coding agent that explores, plans, tests, reviews, and commits its own changes in one repository
- [Researcher](researcher.md): Researcher is a ready-made research agent that searches the web, reads its sources, and writes a cited report into its files folder
- [Coach](coach.md): Coach is a ready-made personal coach for running, cycling, swimming, lifting, team sports, and everyday eating
- [Personal Assistant](personal.md): Personal Assistant is a ready-made agent, named `personal`, that learns about you, keeps your lists and reminders, and plans your day

<!-- docs-tree:end -->
