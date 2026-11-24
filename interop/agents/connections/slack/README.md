# Slack for Link

The Slack connection, `lnk-slack`, lets your agent answer you in Slack.
Your agent runs as a Slack app of your own, and the app answers you and the chats you share with your agent.

<!-- docs-tree:start -->

## Docs

### Pages

- [Slack](docs/agents/connections/slack/README.md): Text your agent on Slack
  - [Slack Guide](docs/agents/connections/slack/guide.md): Create and pair a Slack app, then choose which workspace channels may reach your agent
  - [Delivery Protocol](docs/agents/connections/slack/more/delivery.md): The Slack adapter’s delivery commands connect a shared hosting process to its native service

### More

- [Decisions](docs/agents/connections/slack/decisions.md): Why the Slack module gives each agent an app of its own, and how it hears messages
- [Security](docs/agents/connections/slack/security.md): How a push from Slack is checked on a machine that packs agents, beyond what every webhook's secret gets, which Link's Credentials page covers
- [Troubleshooting](docs/agents/connections/slack/troubleshooting.md): What to do when your agent's Slack app doesn't connect, or its messages don't arrive

<!-- docs-tree:end -->

## Code

- [`bridge.rs`](src/bridge.rs): messages carried for a harness that speaks no Slack
- [`main.rs`](src/main.rs): `lnk-slack connect` and its commands
- [`pushed.rs`](src/pushed.rs): messages pushed by the Events API
