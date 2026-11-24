# Connect Messaging Apps

> Connections Keep Your Agent Within Reach

Connect your agent to a messaging app so you can text the agent from your phone or workspace.

```sh
lnk agent start
lnk agent connect telegram
```

## Choose an App

Use a bot or app of your own on Telegram, Slack, Discord, or Signal. Each app's [connection guide](../../../../../connections/docs/agents/connections/README.md) explains the account, credentials, and pairing steps the app needs.

```sh
lnk agent connect telegram|slack|discord|signal
```

### Pair Your Agent

A running agent keeps answering its other connections and its terminal
while the pairing code waits, and restarts with the new connection once
you're paired. Connecting a connection the agent already has stops the agent
while you pair, since the agent could answer the code itself. So does
linking a connection that makes the agent a device, such as Signal. Each starts
the agent again after. Connecting asks before it stops the agent, and answering
no changes nothing.

### Replace an Expired Code

A pairing code waits ten minutes. An expired code says so. Run
the same `lnk agent connect` again to make a new code.

## Select Another Agent

Give each agent its own bot or app, and use `--agent` to select the agent you want to connect. The connection's setup and conversation history belong to that agent.

```sh
lnk agent connect telegram --agent work
```

## Share a Conversation

Grant a group access to your agent, and end that access, with [Share a Conversation](share/README.md). [Chat](../../chat/README.md) covers conversations in this terminal.

## Receive Service Events

A service or script posts signed events to your agent through a [webhook](webhook/README.md). Each webhook has its own identity and secret, and its posts arrive as messages from a service.

## Understand Pairing and Shared Access

Owner pairing, shared conversations, and signed service events are in [Security](security.md). [Decisions](decisions.md) explains why connections isolate chats and use a bridge for delivery.

## Contribute Here

An adapter for another messaging app lets people reach their agents
where they already talk, beside Telegram, Slack, Discord, and Signal.
Each app's adapter is a module that answers
[the connection protocol](../../../../../connections/contract/docs/agents/connections/more/protocol/README.md),
which says every message, attachment, and pairing step it carries.

<!-- docs-tree:start -->

## Docs

### Pages

- [Share a Conversation](share/README.md): Use `lnk agent share` to let people in a selected Slack channel, Telegram group, or Discord channel talk to your agent
- [Webhooks](webhook/README.md): Use `lnk agent webhook` to let GitHub, a script, or another service post signed events to your agent

### More

- [Decisions](decisions.md): Why connections pair an owner, isolate conversations and delegate delivery through a bridge
- [Security](security.md): How connections establish their owner, bound shared conversations and authenticate service events

<!-- docs-tree:end -->
