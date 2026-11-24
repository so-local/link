# Telegram Guide

Pair a Telegram bot with your account, then choose which groups may reach your agent. BotFather creates the bot and supplies the token that Link asks for.

```sh
lnk agent start
lnk agent connect telegram
```

## Connect it

Make a bot with BotFather and paste its token when `lnk` asks, then
send the bot the one-time code `lnk` prints, within 10 minutes.

```sh
lnk agent connect telegram
```

### Without prompts

From a script, set `LNK_TELEGRAM_TOKEN` to the bot's token in the environment the script runs `lnk` in, and [`lnk agent connect telegram`](../../../../../../../../modules/agents/agent/docs/agents/README.md#connect-your-channels) reads it instead of asking. The [shared connection guide](../../../../../../../../modules/agents/connections/docs/agents/connections/guide.md#connect-it) explains credential inputs.

### On a box

An agent that [moved to a box](../../../../../../../../modules/agents/agent/docs/agents/move/README.md)
is connected on that box, where it runs: open a shell there with
[`lnk box ssh gcp-1`](../../../../../../../../modules/compute/box/docs/services/boxes/ssh.md#open-a-shell-on-a-box) and connect it as above. The box checks the token,
prints the one-time code and waits for it, so only the box reaches
Telegram. A connection made before the move goes with the agent instead.

```sh
lnk agent connect telegram --agent main
```

## Photos and documents

For a harness that takes images and documents (Link Harness), the
photos, documents and voice notes a message holds go to the agent with
its text: PNG, JPEG, GIF or WebP images, PDF or text documents and Ogg
voice notes, up to 10 MiB each and 8 a message, downloaded through the
Bot API (a photo at its largest).

### What it leaves out

Anything else, a video, is named to the agent as left out; sent alone,
it's answered with what the agent reads. For another harness, a message
holding only them is answered "I can read text only, for now."

## While it works

Telegram shows the agent typing, then each tool it calls on a line
above the answer being written, such as `› file read: notes/plan.md`.
By default the finished answer replaces those lines, and the agent's
specification can [keep them as a log](../../../../../../../../modules/agents/connections/docs/agents/connections/guide.md#tool-calls-while-it-works), turn them off, or change the `›` that
leads each line and the `: ` before its detail. Telegram
marks no message with a reaction.

```toml
schema = 1
[connection.telegram]
activity = "keep"
activity_separator = " · "
```

## Formatted answers

The agent answers in Markdown, and Telegram shows it formatted: bold,
italic, strikethrough, code, code blocks, links, and quotes, each in
the HTML Telegram's Bot API reads (`parse_mode` `HTML`). A heading is
bold, a list's items follow bullets, and a table is aligned text in a
code block. Whatever HTML the agent writes itself is shown as text,
and [only web and mail links are linked](../../../../../../../../modules/agents/connections/contract/docs/agents/connections/more/protocol/formatting.md).

### Long answers

Telegram takes at most 4,096 characters in a message, counted in UTF-16
code units, so a longer answer goes in more messages. They're cut
between blocks, lines, or words, never inside a code block's line, and
a code block cut in two is closed and opened again. The limit is in
the adapter's `formatting.toml`.

### Refused formatting

When Telegram can't read a message's HTML, it refuses it as unable to
parse entities. That message is sent once more as plain text, the same
words without their formatting, and never tried a third time.

## In a group

Mention the bot in a group, or reply to it, and it answers there, as a
reply. Each group is a conversation of its own, apart from your direct
chat. In a group with topics (a forum), each topic is a conversation of
its own too, answered in its topic: the agent keeps what was said in
one topic out of the others.

## Share a group

Mention the bot with the one-time code `share` prints, in the group,
within ten minutes. Members can then mention it or reply to it, and the
agent knows their names and who sent each message.

```sh
lnk agent share add telegram
lnk agent share list
lnk agent share remove research --connection telegram
```

### Who stays out

Their direct messages stay private to you. Only a paired user's code
shares a group, and bots and anonymous senders stay out.

### What it gives them

Everyone in a shared group can use everything the agent reaches,
including its files and memory, on your bill: make a separate agent for
separate files. Sharing needs a harness that reads who is speaking, and
group answers on.

## Pushed to a webhook

When the agent runs only while answering, its hosting process carries incoming Telegram messages for it. The [delivery protocol](more/delivery.md) explains the adapter’s commands, retries, and credentials at that boundary.

## Disconnect

`disconnect` forgets Telegram's tokens and restarts your agent if it
runs. The bot stays yours, so delete it with BotFather (`/deletebot`)
if you no longer want it.

```sh
lnk agent disconnect telegram
```

## Environment Inputs

`LNK_TELEGRAM_TOKEN` supplies the bot's token when connecting it.
`LNK_TELEGRAM_API` overrides Telegram's Bot API for isolated tests.
