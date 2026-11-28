# Signal Guide

Link a dedicated Signal number to your agent, then send messages from your own number. Keep the agent’s phone available to scan the linking code.

```sh
lnk agent start
lnk agent connect signal
```

## Connect it

Give your agent a number of its own, such as a second or prepaid SIM.
`connect` asks for the agent's number and yours, then shows a QR code
to scan from the agent's phone (Settings > Linked Devices > Link New
Device). The first time, Link installs `signal-cli` for you.

```sh
lnk agent connect signal
```

### Without prompts

From a script, set `LNK_SIGNAL_NUMBER` to the agent's number and
`LNK_SIGNAL_OWNER` to yours, such as `+15557654321` and `+15551234567`,
in the environment the script runs `lnk` in. [`lnk agent connect signal`](../../../../../../../../modules/agents/agent/docs/agents/connections/connect/README.md#choose-an-app)
reads both instead of asking, and you scan the QR code as before.

## While It Answers

Signal shows typing while your agent prepares its reply. Link refreshes
that indicator every ten seconds, stops it before sending the completed
answer, and never edits an answer already sent.

## Formatted Answers

Signal takes no markup, so the agent's Markdown is sent as plain text
with Signal's text styles beside it: bold, italic, strikethrough, and
monospace. A heading is bold, code and tables are monospace, and a
link's address follows its text. `signal-cli` [takes each style](../../../../../../../../modules/agents/connections/contract/docs/agents/connections/more/protocol/formatting.md) as
`textStyle` on its `send`, `<start>:<length>:<STYLE>`, counted in
UTF-16 code units.

### Long answers

Signal shows at most 2,000 characters of a message whole, so a longer
answer goes in more messages. They're cut between blocks, lines, or
words, never inside a code block's line. The limit is in the adapter's
`formatting.toml`.

## Photos and documents

For a harness that takes images and documents (Link Harness), the
photos and documents a message holds go to the agent with its text:
PNG, JPEG, GIF or WebP images and PDF or text documents, up to 10 MiB
each and 8 a message, read from what `signal-cli` kept of them in the
account's folder.

### What it leaves out

For omitted attachments and harnesses that accept text only, see the [shared attachment handling](../../../../../../../../modules/agents/connections/docs/agents/connections/guide.md#unsupported-attachments).

## Disconnect

`disconnect` removes the saved Signal connection settings and restarts
your agent if it runs. The linked-device account can remain on disk. To finish, unlink it on the agent's phone (Settings > Linked
Devices).

```sh
lnk agent disconnect signal
```

When it doesn't work: [Troubleshooting](troubleshooting.md).

## Environment Inputs

`LNK_SIGNAL_NUMBER` names the agent's number with its country code, and
`LNK_SIGNAL_OWNER` names yours. `LNK_SIGNAL_CLI` selects a test-owned `signal-cli`
program for Link Harness. Its account stays in the agent's connection folder.
