# Security

How a post to [one of your agent's webhooks](README.md) is checked, beyond what every webhook's secret
gets, which Link's [Credentials](../../../../../../../security/docs/config/credentials/webhook.md) page covers. What your agent's bridge does around it, the address on
127.0.0.1 and the ids it keeps, is in [Security of
agents](../../../../../../agent/docs/agents/connections/connect/security.md#webhooks).

## A post

Each post must pass the checks below. Signature and time validation
finish before the content type is inspected.

### Scheme

A webhook accepts only its configured sender's scheme.
[Shared verification](../../../../../../../security/docs/config/credentials/webhook.md#how-its-checked)
owns signature and time checks, and the Link and GitHub signing formulas.

### Cap

A post is at most 1 MiB (`UPDATE_MAX`). The bridge answers 413 to a body
over it, before reading it whole, so `sift` is never handed one: `sift`
bounds its JSON protocol lines separately, refusing an oversized line
with 400, and checks the decoded body cap again itself.

### Its type

A post signed right must carry JSON (`application/json` or
`application/*+json`, whose body is JSON) or text (`text/*`), and is answered 415 for
anything else, a form among them. Its type is looked at only once the
signature holds.

### Keep

A post's dedupe is the id its scheme signs, never one it doesn't:
Link's `link-id`, or the SHA-256 of GitHub's body. GitHub's
`x-github-delivery` isn't signed, so a captured body sent again under a
new one is the same post, refused. The bridge keeps the id 30 days once
your agent has the post, and refuses it again within them; `sift` keeps
nothing.

### Answers

`sift` answers 400 without the time or the id Link's scheme signs, or
for JSON that isn't; 401 without a signature, with a wrong one, or
outside the window; 413 and 415 as above; and 503 for a webhook with no
secret, or that the bridge describes wrongly, its sender among what it
must say, closed; the sender must handle retrying a 503.

## Rolling its secret

The agent bridge supplies current and previous secrets with each post.
The validator accepts either supplied secret without keeping it.
[Rotate a Webhook Secret](../../../../../../agent/docs/agents/connections/connect/webhook/README.md#give-it-a-new-secret)
owns the overlap window, explicit retirement, and restart behavior.
