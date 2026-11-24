# Slack

> Slack’s Newest Coworker Never Logs Off

Text your agent on Slack. Your agent is an app in your workspace and
needs no public address. Your agent answers you and the chats you share
with the app.

<!-- quickstart -->

## Start Your Agent

Start your agent before pairing Slack. Your agent needs a Slack app of its own, and the [connection guide](guide.md) explains how to make one.

```sh
lnk agent start
```

## Connect Your Agent to Slack

`lnk` opens Slack's page for a new app, already filled in. The complete workflow is in [Connect it](guide.md#connect-it).

```sh
lnk agent connect slack
```

## List the Slack Channels Shared With Your Agent

Review the Slack channels that may reach your agent. To grant or remove access, follow the [sharing guide](guide.md#share-a-channel).

```sh
lnk agent share list
```

## Understand and Fix the Slack Connection

Each agent owns its Slack app and listens through Socket Mode. [Decisions](decisions.md) explains both choices. [Security](security.md) covers pairing, shared channels, and signed pushes. When tokens or delivery fail, [Troubleshooting](troubleshooting.md) helps.

## Deliver Messages When a Host Wakes Your Agent

Some hosts wake agents only for their turns. The Slack adapter carries messages for them through its [delivery protocol](more/delivery.md). Pairing and sharing restrictions stay the same as in the regular connection.

<!-- docs-tree:start -->

## Docs

### Pages

- [Slack Guide](guide.md): Create and pair a Slack app, then choose which workspace channels may reach your agent
- [Delivery Protocol](more/delivery.md): The Slack adapter’s delivery commands connect a shared hosting process to its native service

### More

- [Decisions](decisions.md): Why the Slack module gives each agent an app of its own, and how it hears messages
- [Security](security.md): How a push from Slack is checked on a machine that packs agents, beyond what every webhook's secret gets, which Link's Credentials page covers
- [Troubleshooting](troubleshooting.md): What to do when your agent's Slack app doesn't connect, or its messages don't arrive

<!-- docs-tree:end -->
