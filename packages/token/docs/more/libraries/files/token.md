# Token

`link-token` is Link's one access token: a short signed JWT (JSON Web
Token) saying who acts and from which kind of client, never what's
allowed. An issuer issues it, and every resource server it's for
verifies it alone, with the verifiers of the issuers it trusts and no
call to any of them.

## What a Token Holds

An access token is a compact JWS (JSON Web Signature) typed `at+jwt`,
as OAuth names an access token (RFC 9068), made and checked through
[`link-signer`](../../../../../signer/docs/more/libraries/files/signer.md).
It has one shape whoever holds it, a person or a service. Its claims
are declared in the package's `types.toml`, and a resource server
refuses one holding a claim it doesn't know or missing one it needs.

### `iss`

Its issuer, which signed it, as `service:<id>`. It must be the trusted
issuer whose verifier signed it, so no issuer speaks for another.

### `sub`

Its principal, who acts, `user:<id>` or `service:<id>`, as
`link-principal` writes it. Nothing else in it names an actor.

### `client`

The kind of client its principal logged in from, a tag, never inferred
from its other claims:

- **`app`:** a browser or a phone, for a user.
- **`cli`:** an installation's command line, for a user.
- **`service`:** a service, logged in as itself.
- **`box`:** a box, logged in as its own service principal.
- **`wake`:** a wake, acting as its workload's service principal.

An assignment's `via` names `app` or `cli`, so a resource server uses
only the assignments whose `via` holds the access token's kind
(`Checked::via`). A service principal's assignments name no `via`.

### `aud`

Its audience: the one service it's for, or one name a group of
resource servers all accept, so the same access token is passed on
from hop to hop. A resource server refuses an access token whose
audience isn't its own.

### `wkl`

The wake it acts in, `<kind>:<id>@<run>`, its workload and run, present in a wake's access
token alone. It never grants anything: a resource server reads it to
refuse a wake that has ended, and for its logs.

### `iat` and `exp`

When it was made and when it ends, in Unix seconds. A resource server
refuses one that has ended or was made further ahead than clocks drift.

### What a Token Never Holds

An access token lists no actions, no resources, no project, and no
owner. Who may do what is said by assignments, which the resource
server holding a resource reads at each request, so an assignment
removed ends what it gave at the next request, whatever access tokens
are still alive.

## How a Token Is Made

An issuer issues an access token with `Mint::token`, naming who acts,
its kind of client, its audience, the wake when one, and its life. It's
refused, never issued, when its kind of client and principal don't hold
together, or a `wkl` is on anything but a wake's. An access token's
life is `token_life`, five minutes, unless its issuer's settings say
less, and never more than `token_life_max`.

## Prefixes

Every secret IAM makes opens with a prefix naming its type, never who
holds it, so a person reading one knows what it is and a secret scanner
finds it. The prefixes are declared once, in `src/prefixes.toml`, and
whatever reads a secret strips its prefix in one place, so the JWT
inside an access token stays standard for any tool:

| Type | Prefix | Its body |
|---|---|---|
| Access token | `lnk_access_` | A standard JWT, `typ` `at+jwt` |
| Refresh token | `lnk_refresh_` | A JWS its issuer alone takes, `typ` `lnkr` |
| Client secret | `lnk_client_secret_` | Random, ending in a checksum |
| API key | `lnk_api_key_` | Random, ending in a checksum |
| One-time code, a machine's half | `lnk_code_` | Random, ending in a checksum |

## Refresh Tokens

A refresh token is issued only by a login, a browser, a phone, an
installation, or a wake spending its one-time code, and traded at its
issuer alone, only ever for access tokens (`refresh`). It names `iss`,
`sub`, `client`, the kind of client that logged in, `jti`, its own ID,
`wkl` for a wake's, `iat`, and `exp`, and carries no action. Only its own
issuer takes it (`refresh::check`), and the issuer ends one by its
`jti`, as when a login is deleted.

## Random Secrets

A client secret, an API key, and a one-time code's machine half are
random (`secret`): the prefix, 256 random bits as hex, and a checksum,
the first 32 bits of the SHA-256 of the prefix and body, so a scanner
confirms a match offline. `secret::mint` makes one and answers the hash
its taker keeps, the SHA-256 of the whole text, and `secret::read`
checks one's prefix and checksum and answers that hash to look it up
by. No taker keeps the secret itself.

## How a Token Is Checked

A resource server holds a `Trust`: a set of issuers (`Issuers`), each
known by its verifiers, the audience it is, and the longest life it
takes. The set is data the resource server is given, never one issuer
written into its code, so another issuer joins without a code change.
`Trust::check` reads the header first and refuses any `typ` but
`at+jwt`, after stripping `lnk_access_`, so a signature or a refresh
token is never an access token.

### Claims Checked

It finds the trusted issuer whose verifier `kid` names, checks the
signature with it, then the claims: `iss` that issuer, `aud` its own,
the kinds holding together, the life no longer than it takes, and `exp`
still ahead. What passes is a `Checked`, who acts and from which kind
of client, which allows nothing itself.

### New Verifiers

An issuer replaces its signer by publishing the new verifier beside the
old one. A resource server given both (`Issuers::set`) takes access
tokens from either until the old one is dropped.

## Limits

The package's limits are in `src/limits.toml`, read once and checked by
a test: an access token's usual life and the longest any may live.
