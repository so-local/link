# Skill Commands

Skill commands run a skill, a reusable prompt, as a turn of its own,
and write new ones from what the agent just did.

## Run a skill

A skill is a prompt, a folder holding a `SKILL.md`: yours in the
agent's `skills/`, or the repository's in `.agents/skills/` or
`.claude/skills/` of Home and each folder above it up to the
repository's root. `/skill:deploy staging` asks a turn of its own with
what the skill's `SKILL.md` says, then what you said after its name.
[`/skill:list`](../../../../../../agent/docs/agents/tools/use/skill/README.md#see-installed-skills), or `/skills`, names each skill and whose it is, and the
skills section of the instructions names the repository's beside
yours. `[commands] skills` turns both on.

### Read a Skill for a Task

The normal task route reads the matching skill with an offered read tool.
File reads `skills/<name>/SKILL.md` even without a Memory engine. Memory can
read it when configured. A catalog entry lists a skill but does not supply
its instructions. Read later parts too when a response is clipped.

### Loading Evidence

An explicit invocation keeps the original owner input separately from its
expanded message. The owner event's execution evidence identifies the
owned or repository-relative source, its content digest and read range,
the supplied instruction body, and the expanded message. A bounded or
lossy read remains incomplete. This records instruction loading, while
following the instructions is established by the resulting work.

### Which runs by a bare name

`/deploy` runs a skill too. Yours runs by its name even where a command
has it, so a skill of yours named `status` replaces Link's command of
that name. A repository's runs by its bare name only where no command
has it, so a cloned repository can't take over `/stop`. Where yours and
the repository's share a name, yours runs. `/skill:<name>` always runs
the skill, but the skill namespace's own commands, `/skill:list` and
`/skill:write`, stay theirs.

## Learn a skill

`/skill:write` asks a turn of its own, with Link's `learn` skill: the agent
writes a skill from what it just did, `skills/<name>/SKILL.md`, or adds
to one it has, or fixes one that misled it, and says which. What you
send after it, `/skill:write the deploy`, says what to keep. Unlike the
others, it's answered by the model.

### Where it's offered

It writes with its file tool, so it's offered only to an agent whose
file tool is on (`[tool.file] enabled`, on in Link's specification) and whose sandbox file lets it write `skills/` by name, as Link's
does by default ([`lnk agent permission add --files.write skills`](../../../../../../agent/docs/security/sandbox/permissions/README.md#choose-what-your-agent-can-reach) names it again where it was taken away). Elsewhere `/skill:write` is text for the model,
and `/help` doesn't name it.
