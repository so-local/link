# Telegram

> Telegrams Your Agent Answers in Seconds

Text your agent on Telegram. Your agent is a bot you make with
BotFather. Your agent answers you and the groups you share with the bot.

<!-- quickstart -->

## Start Your Agent

Start your agent before pairing Telegram. Your agent needs a Telegram bot of its own, and the [connection guide](guide.md) explains how to make one.

```sh
lnk agent start
```

## Connect Your Agent to Telegram

Make a bot with BotFather and paste its token when `lnk` asks. Then send the bot the one-time code `lnk` prints, within 10 minutes. The complete workflow is in [Connect it](guide.md#connect-it).

```sh
lnk agent connect telegram
```

## List the Telegram Groups Shared With Your Agent

Review the Telegram groups that may reach your agent. To grant or remove access, follow the [sharing guide](guide.md#share-a-group).

```sh
lnk agent share list
```

## Understand the Telegram Connection

Each agent needs its own bot. [Decisions](decisions.md) explains why. [Security](security.md) covers shared groups and authentication when Telegram pushes messages to a webhook.

## Deliver Messages When a Host Wakes Your Agent

Some hosts wake agents only for their turns. The Telegram adapter carries messages for them through its [delivery protocol](more/delivery.md). Pairing and sharing restrictions stay the same as in the regular connection.

<!-- docs-tree:start -->

## Docs

### Pages

- [Telegram Guide](guide.md): Pair a Telegram bot with your account, then choose which groups may reach your agent
- [Delivery Protocol](more/delivery.md): The Telegram adapter’s delivery commands connect a shared hosting process to its native service

### More

- [Decisions](decisions.md): Why the Telegram module gives each agent a bot of its own
- [Security](security.md): How a push from Telegram is checked on a machine that packs agents, beyond what every webhook's secret gets, which Link's Credentials page covers

<!-- docs-tree:end -->
