# Pushed Messages

A machine that packs agents has a connection's messages pushed to it
over HTTPS, or held for it, instead of a bridge polling them. Five
commands do it, none of them run by `lnk` itself.

## `lnk-<connection> hook`

A connection whose `describe` says `pushed` (`lnk-telegram`,
`lnk-slack`). Reads `link_connection_contract::pushed::Hooking` as one
JSON line on stdin (`{"settings", "url", "secret"}`, the connection's
settings with its token) and points the connection's pushes at `url`,
each carrying `secret`. Runs outside every sandbox, where the token is.

### Each connection's secret

Telegram's secret is its `secret_token`. Slack signs with its own
signing secret, in `settings`, which [`lnk agent connect slack`](../../../../../../../agent/docs/agents/README.md#connect-your-channels) doesn't
keep: the caller adds `signing_secret`, and `config_token` to switch the
app itself, used and never kept. Without the signing secret, Slack's
`hook` fails and switches nothing.

### What it prints

It prints `Hooked` (`{"settings", "steps"?}`): `settings`, what `sift`
is handed, none of it secret (who may talk to the agent, the bot's id),
and `steps`, what a person does on the connection's own pages when the
module couldn't make the change itself.

### Its exits

It exits 13 when the connection refused the webhook
(`link_connection_contract::WEBHOOK_REFUSED`: it answered and said no to
the address or its secret, as Telegram's `setWebhook` does), 11 when it
refused the bot's token (`TOKEN_REFUSED`), and nonzero for any other
failure. Slack's never refuses, falling back to `steps` when it can't
switch the app.

## `lnk-<connection> unhook`

The same connections. Reads `Unhooking` (`{"settings"}`) and turns the
pushes off, so `bridge` polls or holds its socket again.

## `lnk-<connection> sift`

A connection whose `describe` says `pushed` or `held`. Reads `Sifting`
JSON lines until stdin closes, and writes a `Verdict` line for each, in
order. It holds no token and calls nothing.

### The lines it reads

A line is `{"settings", "secret", "previous"?, "headers", "body",
"shared"?}`, the headers' names in lowercase, or `{"settings", "say":
{"chat", "text"}}`. `previous` is the secret before `secret` while it
rolls, taken beside it and sent only then (`Sifting::secrets`, for
`link_secretive::inbound`).

### The chats it's shared with

`shared` holds the chats on the connection the agent is shared with, by
the connection's ids, as its specification's `[access] message` says when the
push comes, never kept from `hook`, and sent only when there are some.

### Drop

`{"drop": {"status"}}`: a wrong secret or signature, 401; a stranger's
message, 200, so the connection doesn't push it again; a line it can't
read, 400.

### Answer

`{"answer": {"status", "content_type", "body"}}`: the connection's
check of the address, or a `say` the connection takes in a push's own
response, as Telegram's `sendMessage`.

### Take

`{"take": {"thread", "chat", "dedupe", "from"?}}`, a message for the
agent: its `Link-Thread`, its chat, and the connection's id for the
push, the same on a retry. For a `say`, the machine sends it with
`send`.

### Who a message is from

`from` is `member:<connection>:<user id>`, set when the message is
someone's in a chat the agent is shared with, not the owner's. Only a
connection whose `describe` says `share` sets it, and only for a chat
`shared` names; a machine that packs agents takes it only in a chat
`shared` names.

### The webhook module's

`lnk-webhook-connection` speaks `sift` alone, run by an agent's bridge for its
webhooks, which [`lnk agent webhook`](../../../../../../../agent/docs/agents/connections/connect/webhook/README.md#create-a-webhook) makes, which hands it a webhook's `{"name",
"from"}` as `settings` (`from` `link` or `github`) and its secrets.

## `lnk-<connection> receive`

The same connections as `sift`. Reads `Taking` (`{"bridge",
"update"}`: a `Bridge` as `bridge` reads it, and the update pushed, its
body as JSON or what `hold` gave) and answers it as `bridge` answers a
message it polls, its endpoint `http://` or `unix:<socket>`. It ends
once the answer is sent.

### Its exits

It exits 11 when the connection refused the bot's token
(`TOKEN_REFUSED`), and 12 when it refused the answer itself
(`link_connection_contract::REPLY_REFUSED`): the connection answered the
send and said no, as it does for a chat gone or a bot blocked. A
connection says the same of a request of the module's own it can't
take, so a 12 isn't always the user's doing; any other failure is
nonzero.

## `lnk-<connection> hold`

A connection whose `describe` says `held` (`lnk-discord`), whose
messages come only on a connection held open. Reads `HoldLine`s until
stdin closes, `{"hold": {"key", "settings"}}` (hold that bot's
connection as `key`) or `{"drop": "<key>"}`. One process holds many
bots, resuming a dropped connection where the connection allows. It
answers and sends nothing.

### What it writes

It writes a `Held` line for each message (`{"key", "update"}`) and for
each connection up or down (`{"key", "state": "up" | "down" |
"refused"}`), `refused` being down because the connection refused the
bot's token. A reader takes a state it doesn't know as `down`, never
`up`.
