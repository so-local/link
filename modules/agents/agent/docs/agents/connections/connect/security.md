# Security

How connections establish their owner, bound shared conversations and authenticate service events.

## You and your shared chats

How each connection knows its owner, and who in a shared chat may do
what.

- **Telegram:** `lnk agent connect telegram` prints a one-time code, 8
  characters, about 40 bits. It records the user id of whoever sends it,
  alone as the whole message, to the bot in a private chat within 10
  minutes; a `t.me` link sends it for you. Only someone who sees your
  terminal can. Group messages, other messages and bots are skipped.
- It configures the harness with that allowlist, and some harnesses with
  their owner commands too; the harness refuses everyone else. For Link
  Harness, the bridge refuses them itself: only that user's direct
  messages and mentions in a group reach the harness, and the rest get
  no answer. Errors never print the bot token.
- **Slack:** the same, with the member id of whoever sends the code to
  the app's bot in a direct message. Link reads it with the bot token
  through Slack's Web API, the token in a header; messages from bots,
  edits and older messages are skipped. The harness takes direct
  messages from that member only, and in a connection only their
  mentions of it.
- Its two tokens, bot and app-level, are checked with `auth.test` and
  `apps.connections.open`, which opens nothing. The app runs in Socket
  Mode: the harness connects out to Slack, and nothing on the machine
  listens.
- Those two tokens are all `connect` keeps: the app's signing secret and
  a configuration token (which can edit the user's Slack apps) are only
  for pushed messages, given to `lnk-slack hook` by whatever pushes
  them, and the configuration token is used for the switch and never
  kept.
- **Discord:** the same, with the user id of whoever sends the code to
  the bot in a direct message. Link sees it arrive on Discord's Gateway,
  a websocket, as the bot, with only the direct messages intent, while
  it waits; it connects before showing the code. Messages in servers and
  from bots are skipped. Link keeps that direct message's channel id
  beside the user id: a harness's home channel.
- The harness takes direct messages from that user only, and in a server
  only their mentions of it. Link refuses a bot whose application lacks
  the Message Content intent the harnesses ask for.
- **Signal:** `connect signal` takes the agent's own Signal number,
  never the owner's, and the owner's, typed at the terminal, so whoever
  sees it decides. The harness allows direct messages from the owner's
  number only, and in a group only their mentions of it. It's linked as
  a device, from a QR code. `disconnect signal` stops using it;
  unlinking it on the phone ends it.
- Every harness, and Link's bridge for Link Harness, speaks Signal
  through `signal-cli`, which the Signal module downloads into the
  harness's home only if its SHA-256 is the one pinned in Link's source,
  as is the Java runtime it needs off Linux x86-64.
- For a harness that speaks Signal, `lnk-signal cli install` and `cli
  link` run in that harness's own sandbox, as its adapter's steps do, so
  nothing the harness can write runs outside it. It keeps the account's
  keys in `~/.signal-cli/data` there, where the sandbox holds them and a
  move carries them, as the module says (`lnk-signal cli state`).
- `link` refuses a phone whose Signal is another number than the one
  given.
- In a group, connection or server the agent is in, every harness
  answers only the owner, and only when they mention it: Link writes
  each harness's group settings that way (any group, the owner as its
  only sender, a mention required). On Telegram a reply to the agent
  counts as a mention.
- A chat the owner shared the agent with (`lnk agent share add <connection>`) is the
  exception: there a harness that reads who a message is from answers
  whoever is in it, when they mention it, [as a member](#a-shared-chat), never as the
  owner.
- On Discord a reply counts only when it pings the agent, as a reply
  does unless its sender turns that off. On Signal every harness takes
  only an @mention, not a reply. The owner adds the bot to a group
  themselves.
- **Link's own connection, `link`** (`lnk agent chat send`), has no account,
  bot or pairing: whoever reaches the agent's endpoint the way its
  bridges do is its owner. That takes the endpoint's key for this start,
  in `~/.lnk/safe/agents/<agent>/bridge.json` (600), which only the agent's
  user reads, so another user of the machine can't chat with it.
- The message goes as a bridge's does: on the harness's own connection,
  proved before the key is sent, the thread a plain name (`link:<name>`,
  letters, digits, `-`, `_` and `.`), never `Link-From`.
- The answers in a `link` thread nobody waited for are held in
  `~/.lnk/safe/agents/<agent>/connections/link/held.jsonl` (600, out of the
  harness's reach, the latest 1,000, each cut at 64 KB, each line naming
  its schema), or kept by the
  harness, until a chat on that thread takes them: one at a time per
  thread, under a lock, each said done before it's shown, so two chats
  never show one answer.
- A chat takes only those owed to the chat; one meant elsewhere is the
  bridge's to send. An answer is shown with its control characters taken
  out, so nothing the model writes moves the cursor or sets the
  terminal's title or clipboard; `--json` writes the C1 controls and the
  bidi and zero-width marks as `\u` escapes.
- `--message` puts the message on the command line, which other processes of
  the machine can read; on stdin it isn't. For an agent on a box, the
  message always goes to the box on stdin, never in [`lnk box run`](../../../../../../compute/box/docs/services/boxes/ssh.md#run-lnk-on-a-box)'s
  arguments.
- `lnk agent disconnect <connection>` forgets a connection's tokens.
- Each connection's pairing is a module of its own (`lnk-telegram`,
  `lnk-slack`, `lnk-discord`, `lnk-signal`), run by `lnk agent connect`
  outside the sandbox, as the harness module is. It takes its tokens
  from its variables or the terminal, and hands them back on its stdout,
  a pipe to the harness module, never on a command line.
- Its arguments are the agent's name and the ids of bots other agents
  have, none secret.

### A shared chat

Who in a chat its owner shared the agent with may make it do what.

- **The agent is the cordon.** Whoever is in a chat its owner shared it
  with (`lnk agent share add <connection>`) can make it do all it can: its files, its
  tools and its memory, its owner's bill. Nothing of it is narrowed for
  them; an owner who wants less shared makes a second agent, with only
  those files and its own bot.
- **Only the owner shares.** `lnk agent share add <connection>` runs the connection's
  module (`lnk-<connection> share`) outside the sandbox while the agent is
  stopped, as pairing does.
- It reads only the messages of the users the connection is paired with
  that mention the bot, takes a one-time code
  (`link_connection_contract::pairing_code`, 8 characters) compared in
  constant time, once, within 10 minutes, and says in the chat that it's
  shared.
- Once the code is sent in the chat, `lnk agent share add <connection>` writes the chat
  into the agent's specification,
  `[access] message`, in place and atomically, reading the specification only
  when this user alone writes it; the running bridge never writes it.
  `lnk agent share remove` takes it out the same way and restarts the agent.
- **Never the owner.** A member's message reaches the harness with
  `Link-From: member:<connection>:<user id>` and their name on the connection
  in `Link-From-Name`, words for the model, never who they are. A
  bridge is handed the shared chats only for a harness whose contract
  says it reads `Link-From`; `share` refuses any other, and a harness
  that speaks the connection itself.
- **Still the owner's:** a member's direct message to the bot,
  sharing, removing a shared chat, and approving what the agent asks.

### Questions on your chats

How a question for you reaches your chats, and who can answer it.

- A question is a call of the agent's sandbox that asks first (a tool's
  call, a page to open), or a webhook's question. The agent's bridge
  sends it to your direct chats alone, never a group: the connection's
  `owner` says which they are.
- Where a turn of yours is under way, it asks on that connection; with
  none, on every connection. It shows the question as the dialog does,
  cleaned and cut, so the call's arguments reach the connection's servers.
- Its six-character response code targets a question in Link’s private
  interaction store. The code does not authenticate its sender: the
  connection must establish the owner and match a recorded delivery on
  that agent’s owner chat.
- A confirmation accepts the code alone or `no <code>`. Text and choice
  questions take code-prefixed content, and sets validate each item.
  Telegram, Slack, and Discord can also match your platform reply to a
  delivered question. The bridge catches targeted input before the
  harness, validates its answer shape, and reports an invalid answer
  without treating it as an ordinary prompt.
- The shared interaction engine records the accepted answer and refuses
  late or already resolved responses. A positive permission answer
  authorizes its reviewed effect, pending the source’s current checks
  and application. It does not report that the operation already ran.
  The [owner-question contract](../../../../../connections/contract/docs/agents/connections/more/protocol/owner-questions.md)
  defines targeting and delivery boundaries.
- Whoever holds your account on a connection can answer: a stolen phone's
  Telegram allows what you would. A webhook's question is plain text of
  600 characters at most, with no link in it (no scheme, `www.`, or
  name a chat links by itself, such as `evil.example`), and at most
  `--questions-per-day` of them a day (20), so a webhook's leaked
  secret can't flood your chats or send you somewhere.

## Webhooks

Where a webhook's posts arrive, and how each is checked.

- A webhook's posts reach the agent's bridge on 127.0.0.1 alone, at a
  port kept in its settings. It never listens on another address: a
  public one is a tunnel's (a relay client), which you open
  yourself. Each webhook's address ends in an id of 128 random bits;
  an address with no webhook is answered `404`.
- A body is capped at 1 MiB before anything else is read of it (`413`).
  Then the webhook's module checks its signature, handed nothing but the
  webhook's own secret, on its stdin, and calling nothing: Link's
  scheme, an HMAC-SHA256 over the time, the post's id and the body,
  within 5 minutes, or GitHub's.
- A post signed otherwise is refused (`400` or `401`) and never reaches
  the agent, nor does one signed right that carries neither JSON nor
  text (`415`). The module is [`lnk-webhook-connection`](../../../../../connections/webhook/docs/agents/connections/webhook/security.md).
- A post's id is kept 30 days once the agent's harness has taken it
  (`hooks-seen.json`, 600, written whole, never through a link), and a
  repeat inside them is refused (`401`), as is the same post while the
  first is being handed on. A post the harness refused, or that couldn't
  reach it, is kept nowhere and answered `503`, so its sender's
  redelivery is taken.
- A post its module gives no id is refused (`503`): every post has one,
  and a module can't turn the check off.
- A post that's taken reaches the agent as [a stranger's message](../../chat/security.md#messages-from-others) from
  `webhook:<name>`: quoted, never as
  yours, in its own conversation, with only the tools `[tools]
  strangers` gives, and its answer goes nowhere: the poster is told
  `202` once the harness has taken it, never waiting for its turn to
  end.
- The bridge hands it to the agent itself, through its way in, saying
  who it's from from the webhook's own record: the module that checks
  the signature never holds the harness's endpoint or its secret, and
  nothing it answers can make a post the owner's. `create` refuses an agent
  whose harness can't say who a message is from, or can't keep a
  conversation's tools.
- Each webhook's secret is 32 random bytes, kept with your secrets and
  given only to the agent's bridge, outside its sandbox; it's shown once
  by `create` or `rotate` and never logged. `rotate` makes a new one and keeps
  taking the previous one for 24 hours, its expiry beside it, or until
  `lnk agent webhook rotate <name> --done`, with no restart: the bridge reads each webhook's
  secrets at each post.
- A post of `{"question": "..."}` alone, signed, is [a question for you](#questions-on-your-chats), never a message to the agent: it
  is answered `202` with the question's id. A `GET` of
  `/h/<id>/questions/<question>`, signed with Link's scheme over an
  empty body, answers its status; a webhook GitHub signs takes none.
- The bridge takes at most 64 posts at once, as its way in takes
  messages; a body not read within 10 seconds, or a check not answered
  within 30, is refused.
