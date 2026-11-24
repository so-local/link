# Principal

> Who Acts, or May Act

`link-principal` is who acts, or may, as one type: `user:<id>` or
`service:<id>`, as the account service's API names it. An access
token's subject (`link-api`, whose generated types are these), a role's
holder and a teammate's sender all name a principal with it, so they
can't disagree; `link-api`'s test holds its grammar to the API's.

## Its kinds

`PrincipalKind` is what comes before the first `:`, one of `KINDS`, and
there are two. A `user` is one person: you, or someone a connection
vouches for. A `service` is a program acting on its own: an agent, the
workload `agent:<its ID>`, acts as a service, which holds its roles.
[Why only these two](principal/decisions.md) has the reasons.

## What isn't one

Nothing else is a principal. A machine is where things run: its signer
signs for its user, so a machine's access token names the user as its
subject and the machine in a field of its own. A chat a connection
keeps is a group that may message an agent, never one who acts. A
webhook's caller acts as a service of its own, and a guest who holds a
tunnel's password holds a capability, not a principal.

## Its id

After the kind, an id of 1 to `ID_MAX`, 128, bytes: ASCII letters,
digits and `ID_PUNCTUATION`, `._:/-`. It's opaque: a `:` or `/` in it
is its own, and `Principal::id` gives it back whole.

## On the generic

A `Principal` is `link-id`'s `Id<PrincipalKind>`, [the generic every
stable ID rides](../../../../../id/docs/more/libraries/types/id.md): made
only by parsing or from a kind and an id, so every one is well formed,
and what isn't one, or isn't a kind, is `NotOne`, saying why.
