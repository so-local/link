# Delivery Protocol

The Discord adapter’s delivery commands connect a shared hosting process to its native service. These commands are run by Link; the normal connection workflow stays in the [guide](../guide.md).

## Held for many bots

On this computer, your agent's bridge holds its bot's Gateway, Discord's
websocket, for as long as the agent runs. A machine that packs agents,
which runs one only while it answers, holds the Gateways for it instead,
with three commands of `lnk-discord`.

### Hold the Gateways

`lnk-discord hold` holds many bots' Gateways in one process, with the
bridge's intents, and writes each message one gets. A connection that
drops resumes its session, and Discord sends what it missed. One
Discord refuses for good (the token wrong or revoked, an intent the bot
may not have) is said to be refused, not just down, until the token
changes. It answers nothing and sends nothing.

### Sift each message

`lnk-discord sift` reads each message without the token. One from
anyone you didn't allow, or from the bot itself, is dropped (the bot
known by its `bot_id`, which connecting kept from Discord's
`/users/@me`, so nothing reads the token to learn it), and yours
is kept, as the bridge keeps it (in a server, only a mention of the
bot). So is anyone's mention of the bot in a channel the agent is
shared with, as theirs, by the channels its line names as it comes.

### Answer it

`lnk-discord receive` answers your message, or a shared channel's, as the
bridge does, over Discord's HTTP API, never through the process holding
the Gateway: the agent shown typing, its answer written in as it comes.

### When Discord refuses

When Discord refuses the bot's token, `receive` and `lnk-discord send`
each say so as a failure of its own, apart from any other. So does an
answer Discord refuses, its channel gone or the bot blocked or kept out
of it.
