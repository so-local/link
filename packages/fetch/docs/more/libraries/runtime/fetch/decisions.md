# Fetch Decisions

Why every call of Link's own goes out on [one client](../fetch.md), `link-fetch`, and
why that client reads answers as it does.

## Why does every call of Link's own go out on one client?

`link-fetch` applies one set of rules to Link's HTTP calls: HTTPS except
for permitted local calls, no redirects by default, bounded answers,
timeouts, a common user agent and consistent errors. Each caller states
any required exception explicitly. Callers share the library without sharing another caller's transport exceptions.

### What it leaves out

Agent-facing requests can need different network and response policies.
Their owning transport defines those rules explicitly rather than assuming
that every agent request uses the default account-API policy.

## Why do calls to others' APIs go through it too?

**A credential is as sacred sent to Slack as to Link.** A bot's token,
a cloud's key and an account's access token all ride the same rules:
`https://` alone, no redirect, an error that never prints them. What
differs is said where the client is made, a named rule each
(`outside`, `metadata_server`, `plain_http`, `public_only`,
`follow_redirects`), never a
client built by hand. A service that isn't Link's adds fields without a
new version, so its answers are read open; a value of the wrong type is
still refused.

## Why does a call that reads part of an answer say so?

**Closed is the default, and a part is a choice a review sees.** Some
routes answer more than one caller needs: a balance's level, of all a
console shows. A caller reading two fields of thirty says so where it
calls (`answer_part`), so the rest isn't refused, and every other
answer stays closed.

## Why is a stream given up when it stalls, not when it's long?

**An agent's files take what they take; a dead connection doesn't
move.** A client `streaming` has no timer on the whole call. A body it
sends is watched as it goes, and given up once none of it has gone for
the stall's length, however long it all takes; its answer is then
waited for while the service works on it. An answer it reads is given
up when none of it comes for that long. reqwest's own read timeout
isn't used: it counts from the send to the answer's start, not reset
while a body goes, so it would stop a long upload that's moving.

## Why is every answer closed?

**An answer that isn't its type is refused, never guessed at.** By default, a
field an answer's type doesn't name is refused as `deny_unknown_fields`
would, for every type read, whether it says so or not: `link-fetch`
reads through `serde_ignored`, which finds every field a type passed
over, at any depth, so a type that forgot the attribute is closed all
the same.

## Why doesn't an error quote the answer?

**What's wrong with an answer is said by where, never by what.** serde's
own message quotes the value it couldn't read, and that value may be a
secret, so an answer of the wrong type is named by the call, the kind
of fault and its line and column. A refusal's body is shown, since it's
the service's words to the person, in printable text and at most 512
bytes of it.
