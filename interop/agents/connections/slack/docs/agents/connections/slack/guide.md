# Slack Guide

Create and pair a Slack app, then choose which workspace channels may reach your agent. Pairing uses Socket Mode, an outgoing connection that receives Slack events without a public address.

```sh
lnk agent start
lnk agent connect slack
```

## Connect it

`lnk` opens Slack's page for a new app, already filled in. Create it in
your workspace, then paste the app-level token (Basic Information >
App-Level Tokens, with `connections:write`) and, once you install the
app, its bot token. Send the bot the code in a direct message within 10
minutes. Link opens Socket Mode before displaying the code, receives
your direct message there and acknowledges the event. It does not poll
your direct-message history to pair you.

```sh
lnk agent connect slack
```

### An app made before

If an earlier try made the app, open it at
[api.slack.com/apps](https://api.slack.com/apps) and give `lnk` its
tokens, rather than make another app.

### Without prompts

From a script, set `LNK_SLACK_APP_TOKEN` to the app token (`xapp-...`)
and `LNK_SLACK_BOT_TOKEN` to the bot token (`xoxb-...`) in the
environment the script runs `lnk` in, and [`lnk agent connect slack`](../../../../../../../../modules/agents/agent/docs/agents/README.md#connect-your-channels) reads
them instead of asking. Variables keep the tokens off the command line,
which other users of this machine can see while `lnk` runs, and no flag
takes them.

## Photos and documents

For a harness that takes images and documents (Link Harness), the
photos and documents a message holds go to the agent with its text:
PNG, JPEG, GIF or WebP images and PDF or text documents, up to 10 MiB
each and 8 a message, downloaded with the bot's token from Slack's own
`files.slack.com` alone (the app's `files:read` scope).

### What it leaves out

For omitted attachments and harnesses that accept text only, see the [shared attachment handling](../../../../../../../../modules/agents/connections/docs/agents/connections/guide.md#unsupported-attachments).

## While it works

Slack marks your message with 👀 while the agent answers, then shows
each tool it calls on a line above the answer being written. By default
the finished answer replaces those lines, and the agent's specification can keep
them, turn them off, or [choose another reaction or none](../../../../../../../../modules/agents/connections/docs/agents/connections/guide.md#tool-calls-while-it-works).

```toml
schema = 1
[connection.slack]
working = "hourglass"
activity = "off"
```

## Formatted answers

The agent answers in Markdown, and Slack shows it in its own markup,
mrkdwn: `*bold*`, `_italic_`, `~strike~`, code, code blocks, quotes,
and links as `<url|text>`. A heading is bold, a list's items follow
bullets, and a table is aligned text in a code block. A longer answer
than Slack's 4,000 characters [goes in more messages](../../../../../../../../modules/agents/connections/contract/docs/agents/connections/more/protocol/formatting.md), cut between
blocks, lines, or words, never inside a code block's line.

### Nobody mentioned

The agent's text has `&`, `<`, and `>` escaped, so an answer can't
mention anyone, such as `<!channel>` or `<@U0EVE>`, or hide a link.
The only links are those Link writes from the Markdown's own, and only
to a web or mail address, with any `|`, `<`, or `>` in it
percent-encoded. A Markdown link to anything else shows as text.

## In a channel

Mention the app in a channel and it answers in a thread on your
message. Each Slack thread is a conversation of its own: mention it
again in that thread and it carries on there; mention it in a new
message and it starts afresh. Your direct messages to it are one
conversation.

## Share a channel

Mention the bot with the one-time code `share` prints, in the channel,
within ten minutes. Anyone there in your workspace can then mention it,
and the agent knows their name and who sent each message.

```sh
lnk agent share add slack
lnk agent share list
lnk agent share remove general --connection slack
```

### Who stays out

Their direct messages stay private to you. Only a paired user's code
shares a channel, and bots and members of other workspaces in Slack
Connect stay out.

### What it gives them

Everyone in a shared channel can use everything the agent reaches,
including its files and memory, on your bill: make a separate agent for
separate files. Sharing needs a harness that reads who is speaking, and
group answers on.

## When Slack pushes its messages

When the agent runs only while answering, its hosting process carries incoming Slack messages for it. The [delivery protocol](more/delivery.md) explains the adapter’s commands, retries, and credentials at that boundary.

## Disconnect

`disconnect` forgets Slack's tokens and restarts your agent if it runs.
The app stays yours, so delete it at
[api.slack.com/apps](https://api.slack.com/apps) if you no longer want
it.

```sh
lnk agent disconnect slack
```

When it doesn't work: [Troubleshooting](troubleshooting.md).

## Environment Inputs

`LNK_SLACK_APP_TOKEN` supplies the app-level token, and `LNK_SLACK_BOT_TOKEN`
supplies its bot token. Supply both together when connecting, or leave both
unset for the interactive flow. `LNK_SLACK_API` overrides Slack's Web API for
isolated tests.
