# Access Tokens

An access token identifies a principal and the authority it may exercise at one
audience until an expiry time. The [API Client](../../../../../packages/api/docs/more/libraries/runtime/api-client.md#access-tokens-jwt)
provides Link's token types, signing helpers, and verifier.

## Its form

A token is a compact JWS (JSON Web Signature), with a header, claims, and
signature encoded as base64url parts. The API schemas define the accepted shape
in [`account.yaml`](../../../../../packages/api/schemas/account.yaml).

### The header and `iss`

The header permits `alg` equal to `EdDSA`, `typ` equal to `at+jwt`, and `kid`
naming the signer. `iss` identifies the issuer that the checker trusts.
Unknown header fields or an unrecognized signer are refused.

### `sub`

The principal is `<kind>:<id>`, with `user` and `service` as its declared kinds.
The ID is one Link minted, twelve lowercase letters and digits. A machine can sign for a user,
but the machine itself is not the principal exercising that user's authority.

### `machine`

A user's machine token names its machine. A user's visitor token does not,
and a service token never names a machine. The receiver's requirement tells the verifier
which principal kinds and machine relationship the receiving operation accepts.

### `acc` and `aud`

`acc` names the account whose resources the grants reach. `aud` identifies
one audience. A successful signature check does not prove that a named
resource belongs to that account, so the receiving operation must check ownership.

### `act` and `scope`

`act` can identify a service acting for `sub`. `scope` contains space-separated
grants, each naming an action and optionally its resource. An identity-only
token may carry no grants, and a caller must not infer grants from identity.

### `usr` and `email`

`usr` is the account username recorded by the issuer. The optional `email`
claim carries a verified email in a visitor token. Neither field substitutes
for checking the stable principal, account, and operation's required authority.

### `nonce`, `iat` and `exp`

An optional `nonce` binds a visitor token to the sign-in that requested it.
`iat` and `exp` are issuance and expiry in Unix seconds. `auth_time`, when
present, preserves the original authorization time and cannot be later than `iat`.

## How it's checked

Callers supply trusted public keys and an explicit expectation. The verifier
returns typed claims only after checking the signature, identity constraints,
audience, required grant, and token lifetime.

### The header first

The verifier checks the closed header and Ed25519 signature before interpreting
the claims as authority. A key ID selects only among supplied trusted keys,
rather than asking the token's issuer URL for a key during verification.

### Then the claims

The verifier checks the expected issuer, audience, principal kind, machine
relationship, and any required action and resource. Expiry must be in the
future, within the caller's maximum lifetime, and no further from issuance
than that lifetime. This check allows no clock skew.

### A claim it doesn't know

The typed reader rejects unknown fields and missing required claims.
`auth_time` is optional, but a value after issuance is invalid. A successfully
decoded token still needs the semantic checks the verifier performs.

### The service's keys

The key set holds public verification keys by ID. `from_spec` reads a typed
signer response, and `parse_checked` validates an operator-supplied set.
Keeping those keys current is the caller's responsibility.

### Replacing a key

A verifier can hold overlapping signer sets during rotation. The issuer and
its callers coordinate when a new signer is trusted and when old authority
expires. The library neither distributes signer updates nor revokes a token
merely because a different signer has been introduced.

## Scopes

The schema's `Scope` declaration owns the scope names and their `x-grants`
mapping. The build generates the mapping from scopes to grants from that declaration, so callers
do not maintain another mapping in their signing or verification code.

### `tunnel`

The declared mapping grants `tunnel.open` on `tunnel:*`. This is a grant value
in the shared API contract, and the receiving implementation must still bind
its resource lookup to the token's account.

### `share`

The declared mapping grants `share.make` on `share:*`. A receiving operation
checks the requested action and resource rather than treating the scope's
name as unrestricted access to the account.

### `visit`

The declared mapping grants `tunnel.visit`. A receiving implementation also
checks the resource's access policy and the visitor's sign-in binding.
The signature alone does not establish that access.

### `api`

The schema defines this scope's machine, agent, and identity-management grants.
Use its generated mapping rather than a copied list, and validate each route's
specific action and resource against the resulting grants.

### `identify`

The declared mapping carries no operation grant. `sub` and `acc` identify
the principal and account, and a receiver using this scope must explicitly
choose an identity-only check.

#### Whose agent

A service principal does not prove ownership of an arbitrary resource bearing
the same ID. Check each resource against `acc` before applying a service's
grants, and never interpret account-relative grants as another account's authority.

## Grants

A grant is an action or `<action>@<resource>`. The schema validates the
accepted grammar, and the grant check tests a requested action and resource
against the grants carried by a token.

### An action

An action can be exact, such as `tunnel.open`, or use a trailing wildcard,
such as `tunnel.*` or `*`. A receiver asks for the concrete action it performs
rather than trusting an arbitrary action supplied by the requester.

### A resource

A resource may be exact, such as `tunnel:web`, or end in `*` to match a prefix.
A grant with no resource reaches matching actions across the account's resources.
Resource ownership remains an independent receiving-side check.

### `allows(action, resource)`

`allows` accepts a matching grant and ignores unrelated actions. This lets a
receiver understand the authority it needs without rejecting a token solely
because it also carries a newer action for another operation.

### The kinds it takes

The receiver's requirement limits accepted principal kinds independently of grants.
Its machine requirement also prevents a user's visitor token from standing in
for a machine token, and vice versa.

### What the relay asks

A receiver of tunnel or share grants must declare its concrete required action,
resource, accepted principal kinds, and machine relationship in its requirement.
The generic verifier does not select those policy decisions for a caller.

### What the account service asks

The [API schema](../../../../../packages/api/schemas/account.yaml) declares the
contract for account operations. A route's receiver must validate its named
resource and ownership in addition to verifying the token's signature and grants.

### What an `identify` checker asks

An identity-only receiver supplies no required operation grant. It still checks
issuer, audience, principal kind, machine relationship, and lifetime rather than
accepting an unsigned identity or a token intended for another audience.

### The cases

The schema's `x-allows` cases exercise grant matching in the API library tests.
[Account Stops](../../../../../packages/api/docs/more/libraries/runtime/api-client.md#account-stops)
explains cached revocation cuts, including the original authorization time
retained by refreshed tokens.
