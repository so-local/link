# Connections Guide

Text your agent from the apps you already use. It answers you and no
one else, whichever harness runs it.

```sh
lnk agent start
lnk agent connect telegram
```

## Pick an app

Each app has its own page, saying what to make there and what to paste
into `lnk`, and each gives your agent a bot or app of its own.

| App | What your agent is there |
|---|---|
| [Telegram](../../../../../../interop/agents/connections/telegram/docs/agents/connections/telegram/README.md) | A bot you make with BotFather |
| [Slack](../../../../../../interop/agents/connections/slack/docs/agents/connections/slack/README.md) | An app in your workspace, with no public address |
| [Discord](../../../../../../interop/agents/connections/discord/docs/agents/connections/discord/README.md) | A bot in a server of yours |
| [Signal](../../../../../../interop/agents/connections/signal/docs/agents/connections/signal/README.md) | Your agent's own number, linked from its phone |

## Talk to it here

On this computer, [`lnk agent chat send`](../../../../agent/docs/agents/chat/README.md#talk-to-it-here) talks to your agent with no app at
all, on [Link's own connection](../../../../agent/docs/agents/chat/README.md#talk-to-it-here).

## Let a service post to it

[Webhooks](../../../../agent/docs/agents/connections/connect/webhook/README.md) let GitHub, a script, or anything that signs
post to your agent, which reads each post as a stranger's message.

```sh
lnk agent webhook create deploys
```

## Connect it

`connect` asks for what it needs and pairs you with your agent. Your
agent keeps answering its other ways while it pairs, and restarts with
the connection after.
Each agent needs its own bot or app, so `connect` refuses one that
another agent already has. Connecting a connection the agent already
has asks before starting over. To connect without prompts, set the
connection’s variables, which each app’s page lists. No flag takes a connection token: environment inputs keep its value out of the process’s command-line arguments.

### An Agent That Stops While You Connect

A running agent that could answer the pairing itself, one already on
that connection or one whose harness links the connection as a device
(as Signal does), stops while you connect and starts again after.
`connect` asks first, and a no changes nothing.

### Answers as they are written

Telegram, Slack and Discord show the answer while it is written,
editing the message in place. Signal shows typing, then sends the
complete answer because its editing limits prevent that streaming.
On Slack, the default working reaction is 👀 on the message being
answered; the agent's connection settings can change or disable it.

### Tool calls while it works

While the agent works, Telegram, Slack and Discord show each tool it
calls on a line above the answer being written, with what the call works
on. A line names the tool and then its subject, such as the file read,
the command run, the page fetched, or the query searched.

```text
› file read: notes/plan.md
› run command: cargo test --package web
› web fetch: https://example.com/changelog
› web search

Here's what changed since Friday…
```

The agent's specification chooses how these lines show, with `activity` in
`[connections]` for every connection or `[connection.<name>]` for one.
`replace`, Link's default on every connection, shows the latest eight
while the answer is written, then the finished answer alone. `keep`
leaves every line above the finished answer as a log of what it did.
`off` shows none, and [`/chat:verbose`](../../../../harnesses/link/docs/agents/link/commands/chats.md#show-its-tool-calls) turns them off and on for one
conversation.

```toml
schema = 1
[connections]
activity = "replace"

[connection.telegram]
activity = "keep"
```

Two more opinions set how a line looks. `activity_mark` leads it, `"›"`
by default, and an empty mark leads with nothing. `activity_separator`
stands between the call and its detail, `": "` by default. A call with
no detail shows no separator, as `› web search` above.

```toml
schema = 1
[connection.slack]
activity_mark = "🔧"
activity_separator = " → "
```

Each of Link's tools names the arguments that say what its call works
on, so a line shows them as the agent gave them. A command shows as
typed, its program and then its arguments. An argument a tool declares
sensitive is never shown, and a harness that sends no detail shows the
tool's name alone.

### Reactions while it works

Slack and Discord can mark the message being answered with a reaction,
`working`, and a message that joined a running answer with another,
`steered`. Each names its emoji as Slack does, such as `"eyes"`, and an
empty name turns it off. Slack and Discord both mark with 👀 by default.

```toml
schema = 1
[connection.discord]
working = "hourglass"
steered = "wave"
```

### Replies in groups

Telegram and Discord send a reply to the message that prompted the
agent. Slack answers in a thread, and Signal sends a plain message to
the group. The app's own page describes its setup and sharing rules;
these reply styles stay the same whichever harness answers.

## Photos and Documents

Link passes supported attachments to harnesses that accept them. Each app’s guide names its formats and how they are retrieved, including whether credentials accompany the download.

### Unsupported Attachments

An unsupported attachment is named as omitted so the agent knows it was not read. A message containing only unsupported content receives an explanation of what the agent can read. A text-only harness instead answers an attachment-only message with `I can read text only, for now.`

## Several agents on one app

Each agent is its own bot or app, so several bots on one app are
several agents, each answering its own messages in its own
conversations and on that bot alone.

```sh
lnk agent connect telegram
lnk agent connect telegram --agent work
```

### One bridge for them all

With `shared_bridge = true` in the machine's
[settings](../../../../../../packages/machine/docs/config/installation.md),
one bridge carries every agent's bots on this machine, each still
answered by its own agent.

## Share it with a chat

[Share a Conversation](../../../../agent/docs/agents/connections/connect/share/README.md) explains pairing a shared chat, the access its members receive, and how to end that access. Each app’s guide supplies its own channel or group steps.

## Stop texting it somewhere

`disconnect` forgets the connection's tokens and restarts your agent if
it runs. The bot or app stays yours, and each app's page says how to
delete it.

```sh
lnk agent disconnect slack
```

## The contract

Each app is a module of its own, and what every one answers for
[`lnk agent`](../../../../agent/docs/agents/README.md#agents) is [Interface](../../../contract/docs/agents/connections/more/protocol/README.md).
