# Owner Questions on a Connection

A connection tells Link which of its chats are the owner's own, so the
agent's bridge can ask the owner there what only they can answer. The
owner's reply with the question's code is caught before the agent.

## `lnk-<connection> owner`

A connection with chats Link can ask the owner on (`lnk-telegram`,
`lnk-slack`, `lnk-discord`, `lnk-signal`) answers `owner`. It reads the
`Bridge` as one JSON line on stdin, as `bridge` does, and prints a JSON
array of the owner's direct chats, each as its thread names it after the
connection: `telegram:42`'s `42`.

### Which chats they are

Telegram's are each allowed user's own chat with the bot, whose id is
theirs, and Signal's each allowed number's, its digits. Slack's and
Discord's are the owner's direct conversation with the bot,
`dm_channel`, which `connect` saves; one connected before Link kept it
has none until it's connected again.

### A connection without it

A connection that doesn't answer `owner`, or names no chat, is asked on
by none: the owner answers its agent's questions another way, such as a
dialog or [`lnk sandbox answer`](../../../../../../../../security/sandbox/docs/tooling/sandbox/tools/questions/README.md#answer-what-a-sandbox-asks).

## A reply that answers

Every owner's message a bridge carries passes one function of the
contract (`link_connection_contract::questions::caught`) before it
reaches the harness. A recorded code targets its immutable question on its authorized owner chat. Code alone answers only confirmation. Text and choice replies use code-prefixed content. Sets accept item directives or overall clarification. Invalid targeted input receives a validation notice. Ordinary conversation remains separate. A positive permission response reports that source checks and application are still pending.

### While a turn is under way

The same function records each owner's turn on its chat while the
harness answers it, so the agent's bridge asks that turn's questions
where the owner is. A message joining a running turn is caught first,
so a code typed while the turn waits for its answer never steers it.

### Targeted Platform Replies

Telegram, Slack and Discord record the platform IDs returned by question delivery. An authenticated owner reply to one of those messages submits to that question before the thread queue. Replies in a different chat or from a different agent route have no authority. Signal and CLI retain explicit code targeting. Delivery receipts contain platform message IDs rather than an inferred success from sending text.

### Agent Identity

A question bound to runtime work follows the agent's stable identity when its display name changes. Reusing that name for another agent cannot route an answer to the previous agent's question. Its original requester and immutable review remain attached to the question.

## Work Controls

Explicit `/work` commands and `/stop` enter before busy-thread queues. A bare `/stop` on a chat with no retained work goes to the harness, which says what it stopped, if anything. The host resolves the exact work or requested tree scope and reports acceptance separately from observed execution. `/work status` and `/work list` show retained work. Pause, resume, stop, redirect, queue, steer, archive, unarchive and delegation hold/release use the shared engine. A harness without registered work refuses the control instead of claiming that it took effect.
