# Discord for Link

The Discord connection, `lnk-discord`, lets your agent answer you in
Discord. Your agent runs as a bot of your own, and the bot answers you and the chats you share with your agent.

<!-- docs-tree:start -->

## Docs

### Pages

- [Discord](docs/agents/connections/discord/README.md): Text your agent on Discord
  - [Discord Guide](docs/agents/connections/discord/guide.md): Connect a Discord bot, pair your account, and choose which server channels may reach your agent
  - [Delivery Protocol](docs/agents/connections/discord/more/delivery.md): The Discord adapter’s delivery commands connect a shared hosting process to its native service

### More

- [Decisions](docs/agents/connections/discord/decisions.md): Why the Discord module gives each agent a bot of its own, and how it sees a code arrive
- [Troubleshooting](docs/agents/connections/discord/troubleshooting.md): What to do when your agent's Discord bot doesn't connect or doesn't answer

<!-- docs-tree:end -->

## Code

- [`bridge.rs`](src/bridge.rs): your messages to the harness and back
- [`held.rs`](src/held.rs): many bots' Gateways on one machine
- [`main.rs`](src/main.rs): `lnk-discord connect`, the bot and its pairing
