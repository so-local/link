# Security

How a push from Slack is checked on [a machine that packs agents](guide.md#when-slack-pushes-its-messages),
beyond what every webhook's secret gets, which Link's [Credentials](../../../../../../../../modules/security/docs/config/credentials/webhook.md) page covers. At home the app runs in Socket Mode, and nothing listens.

## Pairing the owner

Link opens Socket Mode before printing the one-time code. It takes the
code only from a person's direct message, ignoring channel messages,
bots and edits, and acknowledges every Socket Mode envelope before
handling it. The wait ends on expiry or connection loss. Pairing
confirms in Slack before saving the owner's member ID and DM channel.

## A shared channel

Only a paired user's mention with the code shares a channel. It expires
after ten minutes and is taken once. Members speak as
`member:slack:<workspace>/<user>`, never as the owner. Both the socket
bridge and signed pushes check the sender's workspace. A member of
another workspace in Slack Connect, a bot, an edit and a member's direct
message are refused. Unsharing removes the channel from the running
bridge's audience. Messages from members never add a remembered
destination for future answers.

## A push

Each push is checked before its body is interpreted as an event, and
refused at the first check it fails. The size cap is checked before
authentication.

### Scheme

Slack signs each push with the app's signing secret:
`X-Slack-Signature: v0=<hex>`, an HMAC-SHA256 over
`v0:<X-Slack-Request-Timestamp>:<body>`. `sift` checks it through
Link's one way in (`link_secretive::inbound`, `Scheme::SLACK`), the
one HMAC in Link, compared in constant time.

### Cap

Slack bodies over `UPDATE_MAX` (1 MiB) receive 413 before
`sift` authenticates its signature or parses JSON.

### Window

A push's timestamp must be within 300 seconds of now, either side, as
Slack advises: a push heard on the way can't be sent again later. One
outside it is answered 401.

### Answers

`sift` answers 401 for a signature missing or wrong, or a time outside
the window; 400 for a time missing or not a number; and 503 with no
signing secret, closed, so Slack tries again. Only then is the body
parsed.

### What's read of the body

Slack's check of the address (`url_verification`) is answered with its
challenge. An event the bridge wouldn't answer (another workspace's, a
stranger's, the bot's own) is dropped with a 200, so Slack doesn't push
it again.

### A repeat

A push Slack sends again, or one replayed inside the window, has the
same `event_id`, which `sift` returns as the push's dedupe: whatever
runs `sift` answers each one once, per app. After the window, the
signed time refuses it.

## Rolling its signing secret

A person rolls it on the app's page at https://api.slack.com/apps: under
Basic Information > App Credentials > Signing Secret, Regenerate, and
Slack signs every push with the new one from then. Then give the new
one as the app's `signing_secret`, the one before kept beside it
(`previous`) until it's dropped.

### While both are given

`sift` takes either while both are given, so a push Slack signed with
the old one, and sends again, is still answered; a push in between the
two steps is refused 401, and Slack tries it again. Once the old one is
dropped it's refused, and a previous secret alone takes nothing. No
restart: each push is checked with the secrets it comes with.
