# Signer

`link-signer` signs with a private half that never leaves its machine
and checks those signatures anywhere with the public half. Every
token Link mints is a compact JWS (JSON Web Signature) that it signs
and checks.

## Signer and Verifier

A **signer** is what a machine keeps to sign with: an Ed25519 private
key, the `ssh` protocol, held as a `SigningSecret` from
`link-secretive` and written to a file only its program reads
(`Signer::keep`, `Signer::read`). Its **verifier** is the public half,
which is all another machine ever holds of it. A verifier is written as
OpenSSH writes a public key, `ssh-ed25519 AAAA…`, so a key made with
`ssh-keygen` is one.

### Real Points Only

Only a real Ed25519 point is a verifier. A small-order or
non-canonical point would check signatures anyone forged, so
`Verifier::ed25519` refuses each one, however it's spelled.

### Fingerprint

Each verifier is named by its **fingerprint**, `SHA256:` and the
unpadded base64 of the SHA-256 of its SSH key blob: what
`ssh-keygen -l` prints for the same key. A checker holds its verifiers
by fingerprint (`Verifiers`), so a signed statement names the one that
checks it and none is tried in turn.

### Protocol

A **protocol** is one way of making a signature and checking it.
`link-signer` has one, `ssh`, an Ed25519 key signing with no network.

## Compact JWS

A compact JWS is three base64url parts, `<header>.<claims>.<signature>`,
the last the Ed25519 signature of the first two. Its header holds
exactly three fields, declared in the package's `types.toml`:

- **`alg`** is `EdDSA`, the only algorithm taken.
- **`typ`** says what the JWS is, so one kind is never taken for
  another: `lnks` a signature, `at+jwt` a token, `lnka` a service
  credential.
- **`kid`** is the fingerprint of the verifier that checks it.

`jws::sign` makes one. Reading one goes in two steps. `Unverified::parse`
reads only the header, refusing another algorithm, another `typ`, an
extra field and anything longer than `jws_max` bytes. `verify` or
`verify_with` then checks the signature, and only the `Verified` result
reads the claims, strictly: an unknown claim, a missing one or one out
of its declared shape is refused.

## Signature

A **signature** is the short statement a signer makes for one exchange
(`signature::sign`), a JWS typed `lnks`. It names `sub`, what it signs
for, which `link-signer` never interprets, and `aud`, the one party it's
for. Its `iat` and `exp` are at most `signature_life`, a minute, apart,
and `jti` is 128 random bits of its own.

### Spent Once

`signature::check` refuses one meant for another party, one ended, one
made further ahead than `clock_skew`, one living longer than a minute,
and one whose `jti` it has already taken before that `exp`. A `Seen`
store keeps each taken ID until it ends, so a signature is spent once,
where it was meant.

## Limits

The package's limits are in `src/limits.toml`, read once and checked
by a test: a signature's life, the longest JWS read, how many taken
IDs a `Seen` store keeps, and the clock skew allowed.
