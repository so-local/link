# Discord Guide

Connect a Discord bot, pair your account, and choose which server channels may reach your agent. The bot needs its own application and the Message Content intent before pairing.

```sh
lnk agent start
lnk agent connect discord
```

## Connect it

Make an application at Discord's
[developer portal](https://discord.com/developers/applications), turn
on its bot's Message Content intent, and paste the bot's token when
`lnk` asks. Add the bot to a server of yours with the link `lnk`
prints, and send it the code in a direct message within 10 minutes.

```sh
lnk agent connect discord
```

### Without prompts

From a script, set `LNK_DISCORD_TOKEN` to the bot's token in the environment the script runs `lnk` in, and [`lnk agent connect discord`](../../../../../../../../modules/agents/agent/docs/agents/connections/connect/README.md#choose-an-app) reads it instead of asking. The [shared connection guide](../../../../../../../../modules/agents/connections/docs/agents/connections/guide.md#connect-it) explains credential inputs.

## Photos and documents

For a harness that takes images and documents (Link Harness), the
photos and documents a message holds go to the agent with its text:
PNG, JPEG, GIF or WebP images and PDF or text documents, up to 10 MiB
each and 8 a message, downloaded from Discord's CDN, which is never sent
the bot's token.

### What it leaves out

For omitted attachments and harnesses that accept text only, see the [shared attachment handling](../../../../../../../../modules/agents/connections/docs/agents/connections/guide.md#unsupported-attachments).

## While it works

Discord shows the agent typing, then each tool it calls on a line above
the answer being written. By default the finished answer replaces those
lines, and the agent's specification can [keep them or turn them off](../../../../../../../../modules/agents/connections/docs/agents/connections/guide.md#tool-calls-while-it-works). Discord
puts 👀 on your message while the agent answers and removes it after,
and the agent's specification can [name another reaction or none](../../../../../../../../modules/agents/connections/docs/agents/connections/guide.md#reactions-while-it-works). Discord
knows the reaction names in the adapter's `reactions.toml`, such as
`eyes`, `hourglass` and `thinking_face`. The bot's invite already asks
for Discord's Add Reactions permission.

```toml
schema = 1
[connection.discord]
working = ""
```

## Formatted answers

The agent answers in Markdown, which Discord reads itself, so the
answer keeps its bold, italic, strikethrough, code, headings, lists,
quotes, and links. Discord draws no tables, so a table is aligned text
in a code block. The agent's own Markdown characters that would mean
something else to Discord are escaped with `\`, and [an answer still
mentions no one](../../../../../../../../modules/agents/connections/contract/docs/agents/connections/more/protocol/formatting.md): not `@everyone`, a role, or a person.

### Long answers

Discord takes at most 2,000 characters in a message, so a longer
answer goes in more messages. They're cut between blocks, lines, or
words, never inside a code block's line, and a code block cut in two
is closed and opened again. The limit is in the adapter's
`formatting.toml`.

## Share a channel

Mention the bot with the code `share` prints, in a channel of a server
it's in, within 10 minutes (`@company 4K7P2QXM`). Only your own mention
counts: nobody else's message is read for the code.

```sh
lnk agent share add discord
```

### Who it answers there

The bot says "Shared with #team" in the channel, and from then on
[anyone there can mention it](../../../../../../../../modules/agents/agent/docs/agents/connections/connect/share/README.md#share-it-with-a-chat) and be answered, as themselves: your agent
is told their user id and the name they go by in the server, never that
it's you.

### What stays out

A share reaches that one channel, and only the people in it:

- **That channel alone:** a thread in it, another channel of the same
  server, and every direct message but yours stay unanswered. A thread
  can't be shared: mentioned there with the code, the bot says so and
  goes on waiting.
- **Unasked, only while shared:** a schedule's answer goes to the
  channel it was set in only while the channel is shared; `unshare`
  ends that too.
- **People only:** another bot's message, a webhook's and Discord's own
  are never answered, shared channel or not.

### End a share

[`lnk agent share remove discord`](../../../../../../../../modules/agents/agent/docs/agents/connections/connect/share/README.md#see-and-end-a-share) ends a channel's share, by its name or its
id, and the bot stops answering anyone there but you.

```sh
lnk agent share remove team --connection discord
```

## Held for many bots

When the agent runs only while answering, its hosting process carries incoming Discord messages for it. The [delivery protocol](more/delivery.md) explains the adapter’s commands, retries, and credentials at that boundary.

## Disconnect

`disconnect` forgets Discord's tokens and restarts your agent if it
runs. The bot stays yours, so delete it at Discord's
[developer portal](https://discord.com/developers/applications) if you
no longer want it.

```sh
lnk agent disconnect discord
```

When it doesn't work: [Troubleshooting](troubleshooting.md).

## Environment Inputs

`LNK_DISCORD_TOKEN` supplies the bot's token when connecting it. `LNK_DISCORD_API`
overrides Discord's HTTP API for isolated tests, including the Gateway address
returned by that API.
