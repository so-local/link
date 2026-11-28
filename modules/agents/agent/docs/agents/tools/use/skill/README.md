# Skills

> Skills You Can Read and Edit

Skills give your agent reusable instructions for a kind of task. Each
skill is a folder following the [Agent Skills](https://agentskills.io)
standard, with a `SKILL.md` describing its name, purpose, and instructions.
Add the folder once, then edit its files as your needs change.

## Add a Skill

Copy a skill from a local folder or a repository URL into your agent's
skills collection. The source folder must contain `SKILL.md`. Adding
from a URL also needs git.

### From a Local Folder

Copy one local skill. The list and remove commands below work on the same copied skill.

```sh
lnk agent skill add ~/Downloads/pdf
```

### From a Repository URL

Alternatively, copy a skill from a repository URL, instead of from a local folder.
The source must contain `SKILL.md`, and this route also needs git.

```sh
lnk agent skill add https://github.com/anthropics/skills/tree/main/skills/pdf
```

The URL example copies the PDF skill from Anthropic's public skills repository.
Review the skill's `SKILL.md` and supporting files before you use it. Local Link installs a copy
of that folder.

### Find Where a Skill Is Saved

The command copies the folder into
`~/Link/Agents/<agent>/agent/skills/<name>/`. Edit the copied `SKILL.md`
and its supporting files there. The collection moves with your agent;
[Agent Configuration](../../../more/settings/README.md#agentskillsnameskillmd)
describes the folder, and [Switch Harnesses](../../../harnesses/use/README.md#who-your-agent-is)
explains what another harness receives.

### Know How the Harness Reads a Skill

A harness decides how skills enter its prompt and how their full
instructions are read. [Link Harness](../../../../../../harnesses/link/docs/agents/harnesses/link/guide.md#give-it-skills)
lists metadata from the next message and reads the selected skill
through memory. An adapter may translate skills into the harness's own
files, so read that harness's documentation for its behavior.

## See Installed Skills

List the agent's skills by name and description to verify what was
installed. The agent's skills folder is also open to you as ordinary
files, so you can review a skill's complete instructions before you use it.

```sh
lnk agent skill list
```

## Remove a Skill

Remove a skill by name when the agent should no longer use its
instructions. You can also delete the skill's folder from the skills collection.
The original folder you added from is a separate copy.

```sh
lnk agent skill remove pdf
```

## Control Scripts and Permissions

A skill may include scripts or instructions to run programs. Installing
a skill grants no extra permission. Its scripts run only when the agent
executes them through a tool, within the sandbox and program permissions
you allowed. [Permissions](../../../../tooling/sandbox/permissions/README.md)
explains how to control that access.

## Understand Skill Design

Skill folders are copied and checked, and their metadata and instructions reach the selected harness. [Decisions](decisions.md) explains why.

<!-- docs-tree:start -->

## Docs

- [Decisions](decisions.md): Why skills are copied, checked and supplied to the harness this way

<!-- docs-tree:end -->
