# Security

How a push from Telegram is checked on [a machine that packs agents](guide.md#pushed-to-a-webhook), beyond what
every webhook's secret gets, which Link's [Credentials](../../../../../../../../modules/security/docs/config/credentials/webhook.md) page covers. On this computer the bot is polled, and nothing listens.

## A shared group

Only a paired user's mention with the code shares a group. It expires
after ten minutes and is taken once. Members speak as
`member:telegram:<user>`, never as the owner. Both polling and authenticated
pushes refuse bots, anonymous senders and a member's direct message.
Unsharing removes the group from the running bridge's audience.
Messages from members never add a remembered destination for future
answers.

## A push

Each push is checked before its body is interpreted as a message, and
refused at the first check it fails.

### Scheme

`hook` sets the bot's webhook with a secret (`secret_token`, 1 to 256
of `A-Z`, `a-z`, `0-9`, `_` and `-`), and Telegram echoes it in each
push's `X-Telegram-Bot-Api-Secret-Token`. `sift` checks it through
Link's one way in (`link_secretive::inbound`, `Scheme::TELEGRAM`),
compared in constant time.

### Cap

Telegram bodies over `UPDATE_MAX` (1 MiB) receive 413 before
`sift` checks the webhook secret or parses JSON.

### Window

Telegram signs nothing, so a push has no time to check: the secret is
the proof, and it travels only over HTTPS, since `hook` refuses any
other address.

### Answers

`sift` answers 401 without the secret or with a wrong one, 503 with no
secret set, closed, so Telegram tries again, 400 for a body that isn't
JSON, and 200 for a message the bridge wouldn't answer (a stranger's, a
bot's, an edit), so Telegram doesn't push it again.

### A repeat

A push Telegram sends again, or one replayed, has the same
`update_id`, which `sift` returns as the push's dedupe: whatever runs
`sift` answers each one once, per bot.

## Rolling its secret

Run `hook` again, with the new secret: Telegram's `setWebhook` takes the
new `secret_token`, and every push from then carries it. Until the
previous one is dropped, `sift` is handed it beside the new one
(`previous`) and takes either, so a push already sent with the old one
is still answered.

### Once it's dropped

Once the previous secret is dropped, a push carrying it is refused, and
a previous secret alone takes nothing. No restart: each push is checked
with the secrets it comes with.
