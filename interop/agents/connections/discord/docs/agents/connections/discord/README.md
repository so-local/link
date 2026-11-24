# Discord

> Discord’s Quietest Member Does the Most

Text your agent on Discord. Your agent is a bot in a server of yours.
Your agent answers you and no one else, unless you share a channel with your agent.

<!-- quickstart -->

## Start Your Agent

Start your agent before pairing Discord. Your agent needs a Discord bot of its own, and the [connection guide](guide.md) explains how to make one.

```sh
lnk agent start
```

## Connect Your Agent to Discord

Make an application at Discord's [developer portal](https://discord.com/developers/applications), turn on its bot's Message Content intent, and paste the bot's token when `lnk` asks. The complete workflow is in [Connect it](guide.md#connect-it).

```sh
lnk agent connect discord
```

## Understand and Fix the Discord Connection

Each agent gets its own bot, and pairing listens for your code. [Decisions](decisions.md) explains both choices. When the bot cannot connect or answer, [Troubleshooting](troubleshooting.md) helps.

## Deliver Messages When a Host Wakes Your Agent

Some hosts wake agents only for their turns. The Discord adapter carries messages for them through its [delivery protocol](more/delivery.md). Pairing and sharing restrictions stay the same as in the regular connection.

<!-- docs-tree:start -->

## Docs

### Pages

- [Discord Guide](guide.md): Connect a Discord bot, pair your account, and choose which server channels may reach your agent
- [Delivery Protocol](more/delivery.md): The Discord adapter’s delivery commands connect a shared hosting process to its native service

### More

- [Decisions](decisions.md): Why the Discord module gives each agent a bot of its own, and how it sees a code arrive
- [Troubleshooting](troubleshooting.md): What to do when your agent's Discord bot doesn't connect or doesn't answer

<!-- docs-tree:end -->
