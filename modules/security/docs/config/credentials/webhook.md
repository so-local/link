# Webhook Secrets

What proves a post from outside comes from the sender that holds the
secret, and the one way every receiver of Link's checks it. List the configured webhook endpoints without printing a webhook secret.

```sh
lnk agent webhook list
```

## What it proves

A post from outside (a connection's push, a payment's, a repository's,
one to an agent) comes from the sender that holds the secret, as that
sender signs: the secret echoed in a header (Telegram's), or an
HMAC-SHA256 over parts of the post (Slack's, Stripe's, GitHub's, and
Link's own).

### Sender's Credential

A webhook's secret is a credential, the sender's, because it authorizes
the sender's posts to one receiver. The sender and the receiver both
hold it, so it's kept and guarded as every secret is, and it rolls as
[Any Secret Rolls with No Restart](#any-secret-rolls-with-no-restart)
says.

## How it's checked

One way in, the Secretive library's inbound check, for every receiver: a
receiver says how its sender signs, as a value (a `Scheme`), and never
writes verification. An `Inbound` holds the scheme, the receiver's cap
and its secrets; checking a post gives the post's id, or a `Refused`
saying why not. Every receiver gets the same rules.

### Capped, and closed by default

The body is capped before anything is read, each receiver stating its
cap, with no default. A receiver without a secret refuses every post,
and nothing builds one that takes anyone.

### Compared in constant time

The secret or its HMAC (at most eight listed signatures read, each 32
bytes, against one HMAC per secret), which is
the Secretive library's HMAC, on `ring`, the one HMAC and the one hex
decoder for a signature in every workspace (a repository check holds
this; an OpenSSH `known_hosts`
line's HMAC-SHA1, matched by [`lnk box`](../../../../compute/box/docs/services/boxes/README.md#boxes), is no signature).

### A signed time, and a replay refused

A signed time is taken only within its window, either side. A
delivery's id, where the sender gives one, and always one it signed
(GitHub signs only its body, so its id is the body's SHA-256, never its
unsigned delivery header), is kept by the receiver's store (`Seen`) for
its window, or for as long as the sender redelivers when it signs no
time: `LONGEST_KEEP`, 30 days, GitHub's, the longest any sender
redelivers.

#### Kept by name

It's kept under the receiver's name and its scheme's, so one sender's
ids never stand for another's in a shared store; and a scheme that
signs no time takes nothing with a store that forgets at a restart.

### Any secret rolls with no restart

The current secret and the one before it are both tried, so the sender
may switch at any moment, and a previous secret alone takes nothing.

## One answer for each refusal

Each `Refused` has one answer, and a refusal is logged by its kind,
never with the body.

### 503

When the receiver has no secret yet (`Unconfigured`) or its store can't
say (`Unavailable`), so the sender retries.

### 413 and 400

413 for a body over the cap (`TooBig`), and 400 for a missing or
unreadable time or id (`Malformed`).

### 401

For a missing or wrong signature (`Unsigned`, `BadSignature`), a stale
time (`Stale`) or a replay (`Replayed`).

## Link's own scheme

Link's scheme is an HMAC over `link webhook`, the time, the id and the
body, each after a NUL, in the headers `link-time`, `link-id` and
`link-signature: v1=<hex>`; a 300-second window, and the id required.

### GitHub’s Scheme

GitHub's scheme checks `x-hub-signature-256: sha256=<hex>` against an
HMAC-SHA256 of the body alone. GitHub signs neither a timestamp nor its
delivery header. The body digest is therefore the replay identity, as
[Signed Time and Replay](#a-signed-time-and-a-replay-refused) explains.
