# Leverage Link Harness

Link Harness is Local Link's own harness, and it gives you an agent you can
read, change, and measure. Switch your agent to Link Harness, then make the
agent yours in a few steps.

```sh
lnk agent harness set link
```

## Switch to Link Harness

Your agent keeps its model, files, and connections when it moves to Link
Harness, and you can switch back the same way.
[Switch to it, and back](harnesses/link/guide.md#switch-to-it-and-back) has the
whole workflow, and [Link Harness](harnesses/link/README.md) says what the
harness is.

## Edit the Instructions and Skills

Your agent's instructions and skills are plain files that Link Harness reads
on every turn. Edit them yourself, or ask your agent to edit them. An agent
named `main` looks like this on your disk.

```text
~/Link/Agents/main/
  home/                 your files the agent works in
  agent/
    instructions/
      instruction.md    your standing instructions
      persona.md        who your agent is
      owner.md          who you are
    skills/<name>/SKILL.md
    memory/             notes and memory's index
    chats/              each conversation, a log you can read
    schedule/           scheduled jobs
~/Link/Safe/Agents/main/
  agent.toml            your agent's specification
  sandbox.toml          what your agent may reach
```


[Make it yours](harnesses/link/guide.md#make-it-yours) shows each file and what
it changes.

## See the Prompt Your Agent Gets

Link Harness shows the prompt it builds from your agent's files, tools, and
skills, so nothing your agent reads is hidden from you.

```sh
lnk agent show --instructions
```

## Command Your Agent From Any Chat

Slash messages such as `/model` and `/skills` work in the terminal and in
every messaging app. [Commands](harnesses/link/commands/README.md) lists every
command and its familiar spellings.

## Send Images, Documents, and Voice Notes

Link Harness reads the images, documents, and voice notes a message holds, and
keeps each as a file in the conversation's folder.
[Images, Documents and Voice Notes](harnesses/link/media/README.md) says what
each kind needs.

## Keep Conversations Your Way

Link Harness keeps every conversation in your agent's files, as a log you can
read, a smaller file, or a database.
[Conversations](harnesses/link/conversations.md) shows how to choose and how
much of each conversation the model sees.

## Compare Link Harness

Controlled benchmarks compare the startup and idle memory of Link Harness with
Hermes and OpenClaw. [Harness Benchmarks](harnesses/link/benchmarks.md) states
the conditions and the scope of every measurement.
