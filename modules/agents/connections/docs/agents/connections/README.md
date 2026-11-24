# Connections

> Connections for Your Agent’s Chats

Text your agent from the apps you already use. Your agent answers you
and no one else, whichever harness runs it.

<!-- quickstart -->

## Start Your Agent

Start the agent that will receive your messages. The [connections guide](guide.md) lists the supported apps and the account or bot each one needs.

```sh
lnk agent start
```

## Connect Your Agent to a Chat App

Connect your running agent to [Telegram](../../../../../../interop/agents/connections/telegram/docs/agents/connections/telegram/README.md), [Slack](../../../../../../interop/agents/connections/slack/docs/agents/connections/slack/README.md), [Discord](../../../../../../interop/agents/connections/discord/docs/agents/connections/discord/README.md), or [Signal](../../../../../../interop/agents/connections/signal/docs/agents/connections/signal/README.md). Local Link prompts you for the bot credentials and confirms the bot is yours. [Connect Your Channels](../../../../agent/docs/agents/connections/connect/README.md) also covers disconnecting a channel and sharing a chat with other people.

```sh
lnk agent connect telegram|slack|discord|signal
```

## Let a Service Post to Your Agent

Webhooks let GitHub, a script, or anything that signs its posts reach your agent. Your agent reads each post as a message from a stranger. The [webhook module](../../../webhook/docs/agents/connections/webhook/README.md) checks each post before it reaches your agent, and [Webhooks](../../../../agent/docs/agents/connections/connect/webhook/README.md) explains the full workflow.

```sh
lnk agent webhook create deploys
```

## List the Chats Allowed to Reach Your Agent

Review which chats may reach your agent. Granting access takes a one-time code, as [Share It With a Chat](../../../../agent/docs/agents/connections/connect/share/README.md#share-it-with-a-chat) explains.

```sh
lnk agent share list
```

## Build a Connection Module

A connection module pairs an account with your agent and carries messages in both directions. The [interface](../../../contract/docs/agents/connections/more/protocol/README.md) lists the commands every connection module answers. [Connection Policy](../../../contract/policy/docs/agents/connections/more/protocol/policy.md) defines the shared settings and tool restrictions. [Owner Questions](../../../contract/docs/agents/connections/more/protocol/owner-questions.md) describes approvals carried through a chat.

<!-- docs-tree:start -->

## Docs

- [Connections Guide](guide.md): Text your agent from the apps you already use

<!-- docs-tree:end -->
