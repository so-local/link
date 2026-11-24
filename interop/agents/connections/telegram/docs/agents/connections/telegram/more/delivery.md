# Delivery Protocol

The Telegram adapter’s delivery commands connect a shared hosting process to its native service. These commands are run by Link; the normal connection workflow stays in the [guide](../guide.md).

## Pushed to a webhook

On this computer, your agent's bot is polled: `lnk` asks Telegram for
its messages. A machine that packs agents, which runs one only while it
answers, has the messages pushed to it instead, with four commands of
`lnk-telegram`.

### Set the webhook

`lnk-telegram hook` sets the bot's webhook, outside every sandbox,
where the token is: Telegram pushes the bot's messages to an `https://`
address, each carrying a secret (`secret_token`). Messages waiting are
kept. A harness in a sandbox can't set the webhook itself: its proxy
refuses `setWebhook`, `logOut` and `close`.

### Sift each push

`lnk-telegram sift` reads each push without the token. One without the
secret is refused ([Security](../security.md) has how, and how the secret
rolls), one from anyone you didn't allow is dropped, and yours or a
shared group's member's is kept, as the bridge keeps it (in a group,
only a mention of the bot or a reply to it).

### Answer it

`lnk-telegram receive` answers your message as the bridge does: the agent
shown typing, its answer written in as it comes.

### When Telegram refuses

When Telegram refuses the bot's token (revoked with BotFather, say),
`receive` and `lnk-telegram send` each say so as a failure of its own,
apart from any other. So does an answer Telegram refuses, the chat gone
or the bot blocked in it.

### Poll it again

`lnk-telegram unhook` deletes the webhook, so the bot is polled again.
While the webhook is set, Telegram won't let anything poll the bot, and
`lnk` says so:

```text output
this bot's messages are pushed to a webhook (the agent moved to a machine that packs agents?): lnk agent move brings it back
```
