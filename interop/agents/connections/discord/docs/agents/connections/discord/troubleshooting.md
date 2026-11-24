# Troubleshooting

What to do when your agent's [Discord bot](README.md) doesn't connect or doesn't
answer. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk agent show
```

## `connect discord` asks for Message Content

Link requires the bot's Message Content intent before connecting it. On the bot's page at Discord's developer portal, turn on
Message Content Intent, then connect again.

## You can't message the bot

The bot can be messaged only once it's in a server you're in. Add it to
your server with the link [`lnk agent connect discord`](../../../../../../../../modules/agents/agent/docs/agents/README.md#connect-your-channels) printed, then
send it the code in a direct message.
