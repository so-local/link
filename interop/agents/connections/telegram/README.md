# Telegram for Link

The Telegram connection, `lnk-telegram`, lets your agent answer you in
Telegram. Your agent runs as a bot of its own, and the bot answers you and the chats you share with your agent.

<!-- docs-tree:start -->

## Docs

### Pages

- [Telegram](docs/agents/connections/telegram/README.md): Text your agent on Telegram
  - [Telegram Guide](docs/agents/connections/telegram/guide.md): Pair a Telegram bot with your account, then choose which groups may reach your agent
  - [Delivery Protocol](docs/agents/connections/telegram/more/delivery.md): The Telegram adapter’s delivery commands connect a shared hosting process to its native service

### More

- [Decisions](docs/agents/connections/telegram/decisions.md): Why the Telegram module gives each agent a bot of its own
- [Security](docs/agents/connections/telegram/security.md): How a push from Telegram is checked on a machine that packs agents, beyond what every webhook's secret gets, which Link's Credentials page covers

<!-- docs-tree:end -->

## Code

- [`bridge.rs`](src/bridge.rs): messages carried for a harness that speaks no Telegram
- [`main.rs`](src/main.rs): `lnk-telegram connect` and its commands
- [`pushed.rs`](src/pushed.rs): messages pushed to the bot's webhook
