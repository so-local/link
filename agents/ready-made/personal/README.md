# Personal Assistant

Personal Assistant is a ready-made agent, named `personal`, that learns
about you, keeps your lists and reminders, and plans your day. The full
guide is [Personal Assistant](../../docs/agents/ready-made/personal.md) in Local Link's docs.

## Who Personal Assistant Is For

Personal Assistant is for anyone who wants one place to tell what
matters and be reminded of it at the right time. The agent keeps what it
learns in plain Markdown files, one for each part of your life, such as
people, work, health, money, and dates, with to-do and shopping lists.
The agent never records a password, card number, or code, and keeps what
it knows for you alone.

## What Personal Assistant Comes With

Personal Assistant runs on Link Harness and stays awake, so reminders
fire without you. Its model is Link's default, `claude-haiku-5-5` on
Anthropic, the current Haiku.

- **Tools:** file, scheduler, memory, web, and a browser of its own that needs Chromium, Google Chrome, Microsoft Edge, or Brave.
- **Skills:** `learn-about-you`, `recall`, `remind`, `plan-day`, `lists`, and `weekly-review`.
- **Home:** `about-me.md`, `people.md`, `work.md`, `health.md`, `home.md`, `money.md`, `interests.md`, `routines.md`, `dates.md`, `reminders.md`, `lists/`, and `notes/`.

## Bring Personal Assistant In

Download `link-personal.tar.gz` from Local Link's
[latest release](https://github.com/so-local/link-releases/releases/latest), then import it from the folder you saved it in.
Import checks the whole
archive, asks whether to keep the name `personal`, and places the agent in
`~/Link/Agents/personal/`, with its settings in `~/Link/Safe/Agents/personal/`.
Nothing is installed, connected, or started until you say so.

```sh
lnk archive import link-personal.tar.gz
```

From a checkout of Local Link's repository, pack this folder as the
release does, then import `~/Link/Archive/personal.tar.gz`.

```sh
lnk archive export --from agents/ready-made/personal
```

## Start Personal Assistant and Chat

Start the agent, chat, and connect the messaging app you already use.
On its first chat the agent asks your name, city, and how you like
answers. Ask for a reminder in your own words, such as
`Remind me to call Ana on Friday at six`, and the agent offers a daily
morning brief once it knows your time zone.

```sh
lnk agent start --agent personal
lnk agent chat --agent personal
lnk agent connect --agent personal telegram|slack|discord|signal
```

## What Personal Assistant Can Reach

The agent's sandbox writes only its files folder and its own
instructions and skills, reaches its model and the web through its
proxy, calls Link Web, and runs one program, `sh`, for a shell's
builtins. Its browser asks you before it books, buys, signs up, or sends
anything.

```toml
# ~/Link/Safe/Agents/personal/sandbox.toml
schema = 1

[files]
write = [".", "instructions", "skills"]

[internet]
hosts = ["*"]

[commands]
allow = ["sh"]

[tools]
names = ["web"]
```

```sh
lnk agent show --agent personal --permissions
```


## Make Personal Assistant Your Own

**Personal Assistant learns the life you live.** An imported agent is yours. Edit its standing instructions in
`~/Link/Agents/personal/agent/instructions/instruction.md`, or ask the agent
to, since its sandbox writes them. Add or remove skills and tools, and
change one key of its specification with a field flag on
`lnk agent start`, which the agent keeps.

```sh
lnk agent skill list --agent personal
lnk agent tool add memory --agent personal
lnk agent spec show personal
```

A part of your life the Home lacks is a new file, with a line in the Home's `README.md` saying what it holds.
[Make an Agent Your Own](../../docs/agents/ready-made/README.md#make-an-agent-your-own)
covers each change, and
[Share an Agent](../../../modules/agents/agent/docs/agents/share/export.md#share-your-agent)
shows how to export your version for others.

## This Folder

This folder is an archive kept as a folder. `archive.toml` names the
agent and lays out its folders: `Agents/personal/` holds its instructions,
skills, and Home, and `Safe/Agents/personal/` holds its `agent.toml` and
`sandbox.toml`. This README sits beside `archive.toml`, travels in the
released archive, and is left out on import, so the agent's files stay
exactly what the archive names. The Home starts blank, with headings and
empty fields only, and fills as you talk to the agent.
