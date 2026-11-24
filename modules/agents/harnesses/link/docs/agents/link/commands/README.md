# Commands

> Commands Are Slash Messages Link Harness Runs

A command is an owner's slash message that Link Harness handles.
The primary action names are in [Namespace Commands](namespace-commands.md).
Familiar spellings resolve to those same actions, with no separate
implementation, as [Command Aliases](aliases.md) lists.

## Choose Which Commands Are On

Local built-ins use their agent's `[commands]` switches, such as
`[commands] create = true`. Route-backed commands also require a grant
and an available provider. An unavailable command is text for the model.
A connection sends a command as text, so each works on every connection and
with [`lnk agent chat send --message "/help"`](../../../../../../agent/docs/agents/chat/README.md#talk-to-it-here).

### Label command answers

Set `command_response` directly in `[behavior]` in `~/Link/Safe/Agents/<agent>/agent.toml` to label harness
answers, for example `command_response = "🤖 Automated Response"`. The
label appears on its own line before the command's answer. The default
is empty. Ordinary model replies and commands that ask a model question
keep their own output.

### Model commands

`/model:get` shows this conversation's model and effort; `/model:help`
lists its enabled model commands. Both answer without a model turn.
`/model:set <id>` changes the conversation's model, and `/model` shows
the same information as `/model:get` when given no arguments.

## Run Commands from the Command Line

Conversations and retained work are controlled by [Chat Commands](chats.md).
A conversation's model and effort change with [Model Commands](models.md),
reusable prompts run and are written with [Skill Commands](skills.md), and
background work is managed through the task namespace with
[Task Commands](tasks.md).

### Command-Line Controls

Common controls also have CLI forms: `compact`, `create`, `export`,
`goal`, `resume` and `stop` by their own names, `/chat:status` as
[`lnk agent chat show <chat> --status`](../../../../../../agent/docs/agents/chat/README.md#talk-to-it-here),
`/chat:rename` and `/chat:verbose` as [`lnk agent chat set name`](../../../../../../agent/docs/agents/chat/README.md#manage-history) and
`set verbose`, and `/chat:ask`, `/chat:queue` and `/chat:steer` as
`lnk agent chat send` with `--aside`, `--queue` or `--steer`. Give
`--chat <name>` to select their conversation.

#### Sending and Choosing a Model

For example,
[`lnk agent chat send --queue --message "look at the tests next"`](../../../../../../agent/docs/agents/chat/README.md#control-a-running-turn) sends
[`/chat:queue look at the tests next`](../../../../../../agent/docs/agents/chat/README.md#control-a-running-turn) through the same command handler.
[`lnk agent chat send --aside --message "what changed?"`](../../../../../../agent/docs/agents/chat/README.md#control-a-running-turn) asks a side question. Model selection
uses [`lnk agent model set <id>`](../../../../../../agent/docs/agents/models/agent/README.md#switch-in-one-command); with `--chat <name>` it sends
`/model:set <id>` for that conversation.

## List Commands with `/help`

`/help` lists the commands that are on, and says what each is for. It
doesn't name a command the agent's specification leaves off, or one this agent
can't run, such as `/skill:write` without its file tool.

## Inspect its work

[`/chat:status`](../../../../../../agent/docs/agents/chat/README.md#goals-and-visibility) shows the model, its provider, the chat,
and the conversation's messages and tokens. It answers at once, without asking
the model:

```text
Model: claude-haiku-5-5 (anthropic)
Chat: telegram:42
Conversation: 6 messages since /chat:create, about 1840 of 128000 tokens
```

### What `/chat:status` Counts

`/chat:status` counts your messages and the agent's answers from the chat's last
[`/chat:create`](../../../../../../agent/docs/agents/chat/README.md#manage-history) on, or from its start, and the tokens they take of the window a
turn fills.

## Start a new conversation

Send `/chat:create` on any chat. On the command line, [`lnk agent chat start`](../../../../../../agent/docs/agents/chat/README.md#threads-and-scripts) opens a fresh conversation instead. The harness answers
at once, without asking the model, and your next message starts a
conversation of its own. The model is sent nothing from before the
`/chat:create`, not even a summary of it.

### The chat stays the same

The chat's folder, its `events.jsonl`, and its `conversation.md` keep
everything before the `/chat:create`, and memory still searches it. The
command is kept there too, as `/chat:create`, an event of the role `command`, and in
`conversation.md` it is yours.

### Finish an Answer Already Under Way

An answer already being written when you send `/chat:create` finishes as
it began. The next one starts after it.

## Stop an answer

[`/chat:stop`](../../../../../../agent/docs/agents/chat/README.md#control-a-running-turn) ends the answer being written in the chat it's sent on, at
once. What it wrote so far is kept, and sent, marked cut short, and any
call it was making is cancelled. With no answer being written, the command
says so. The command is `session/cancel` of the [Agent Client Protocol (ACP)](https://agentclientprotocol.com), sent as text, so it works
on every connection.

## Pursue a goal

[`/chat:goal <condition>`](../../../../../../agent/docs/agents/chat/README.md#goals-and-visibility) sets the chat's goal and asks a turn toward it.
After each answer the agent's model judges, asked alone, whether it
holds. While it doesn't, the agent takes another turn on its own, its
answer sent to the chat as an answer nobody waited for is. It stops
when the goal holds, when it's judged out of reach, or at its specification's
`[goals]` limits, `turns` and `tokens`, whichever comes first.

### `/chat:goal` and `/chat:goal --clear`

`/chat:goal` shows the goal and what it has spent, or how the last one
ended. `/chat:goal --clear` drops the goal, and `/chat:create` does too. There's no `/chat:goal`
where the specification turns off `[commands] goal`, or leaves out `[goals]
turns`.

## Who Can Run a Command

Only your own message runs a command. A message from anyone else is
text for the model, whatever it says. That includes a person in a chat
the agent is shared with, a schedule, a subagent, a teammate, or a
background command, each of which comes with who sent it (`Link-From`).
The reasons are in [Decisions](decisions.md).

### Messages That Count as Text

A message is text, too, when it holds an image or a document, when
something follows a command that takes nothing after it (`/chat:create
please`), or when no command has its name.

<!-- docs-tree:start -->

## Docs

### Pages

- [Namespace Commands](namespace-commands.md): Namespace commands are the primary names for an agent's actions
- [Command Aliases](aliases.md): Command aliases let you keep familiar spellings when coming from another harness
- [Chat Commands](chats.md): Chat commands manage a chat's conversations and what its next turn is sent: the `chat` namespace's actions beyond `/chat:create`, `/chat:stop`, `/chat:status` and `/chat:goal`, which Commands explains
- [Model Commands](models.md): Model commands show and switch the model a conversation runs on, how hard it thinks and whether it's asked fast, without a restart
- [Skill Commands](skills.md): Skill commands run a skill, a reusable prompt, as a turn of its own, and write new ones from what the agent just did
- [Task Commands](tasks.md): Task commands list and manage the agent's work through its task namespace: background commands, subagents and schedules

### More

- [Decisions](decisions.md): Why Link Harness's commands work the way they do

<!-- docs-tree:end -->
