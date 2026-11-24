# Make an Agent Worth Sharing

Get the most out of an agent by shaping its instructions, skills, tools, and Home, whether you started it from scratch, imported it, or mean to give it away. An agent is plain files on this machine, which you edit yourself or ask the agent to edit.

## Give the Agent One Job

An agent does best with one job it knows well, such as coaching your
training or keeping your reminders. A narrow job lets its instructions
be specific, its skills cover the work, and its sandbox open only what
the work needs. Someone you share it with knows at once what it's for.
Make a second agent for a second job, as
[Create Another Agent](../start/create.md) shows.

## Write Instructions

An agent's standing instructions are
`~/Link/Agents/<name>/agent/instructions/instruction.md`, what it always
does, with `persona.md` beside them for who it is. Its harness reads them
into every prompt, as
[Link Harness](../../../../harnesses/link/docs/agents/link/guide.md#tell-it-who-to-be)
shows. Write rules the agent can follow, such as which file to update
and how short a reply is, rather than hopes about its character.

## Add Skills for Recurring Work

A skill is a folder in `~/Link/Agents/<name>/agent/skills/` holding a
`SKILL.md`, read when its situation comes up. A skill for each task the
agent repeats, saying which files to read and write and in what format,
keeps the work the same every time. `lnk agent skill add` copies one in
from a folder or a URL, and [Skills](../tools/use/skill/README.md)
explains how one is written.

```sh
lnk agent skill remove team-sport --agent coach
```

## Choose Tools and Reach

An agent's tools are the ones its settings turn on and its sandbox file
grants. Give it only the tools its job uses, since each one is something
it can do without asking. Adding or removing one of Local Link's tools
keeps whatever the tool saved, and [Tools and Skills](../tools/use/README.md)
describes each. [Permissions](../../security/sandbox/permissions/README.md)
chooses the folders and hosts it reaches.

```sh
lnk agent tool add memory --agent coach
lnk agent tool remove memory --agent coach
```

## Keep Records in the Home

An agent's Home is its files folder, `~/Link/Agents/<name>/home` unless
you name another. An agent that keeps its records there, in files a
`README.md` describes, builds every answer on what really happened, and
you can read and fix each record yourself. Before you share it, leave
its Home blank, with headings and empty fields, so a recipient's agent
never takes your example for their data.

## Pick a Model

A coding agent does best on a strong model, and most other jobs run well
on a fast, cheaper one. Each agent keeps its own model, so one agent can
use a hosted model while another runs on your computer.
[Switch Models](../models/agent/README.md) covers every provider.

## Ask the Agent to Improve

The quickest way to a better agent is to tell it what went wrong and ask
it to change its own instructions or skills. Its sandbox decides
whether it may, and the files it changes stay plain files you can read
and undo. Ask it to say in one line what it changed.

## Test as a Stranger Would

Before you share an agent, import your own archive under another name
and start it fresh, as its recipient will. Delete `owner.md`, which holds
who you are, and anything private in its Home first. What the new copy
gets wrong without your history is what to fix in its instructions and
skills, and [Share Your Agent](export.md) says what the archive carries.

```sh
lnk archive import ~/Link/Archive/main.tar.gz --select agent.main=agent.work
lnk agent start --agent work
```

<!-- docs-tree:start -->
<!-- docs-tree:end -->
