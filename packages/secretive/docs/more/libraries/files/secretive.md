# Secretive

> Secret Values Stay Out of Ordinary Output

`link-secretive` holds every secret Link holds, in one of its types
by where it may go, so a wrong way doesn't compile. Beside them are
shared HMAC and inbound-post verification, and process guards for programs
that hold secrets.

## Its types

Each type says where its secret may go: nowhere, to a program by name,
into a signature, or sealed to one recipient.

### `OpaqueSecret`

One Link uses and nobody reads, such as an agent's proof secret or a
service's signing secret. It signs and checks; there is no way to its
bytes.

### `Secret`

A value another program or service must be given, such as a model's
API key, a connection's token or a password. It's read only by asking for
it by name, `Secret::expose`, at a place a review sees;
it's never serialized but by `bare`, never copied but by
`Secret::duplicate`.

### `SigningSecret`

An Ed25519 key pair's private half, such as a machine's key, which
signs and says its public half. `keep` returns its private PKCS #8 document
as a `Secret` for reviewed persistence; `from_kept` reads it back.

### `Sealed`

Either, encrypted to one recipient, the only form that's saved or sent
as it is: HPKE (RFC 9180) in its base mode, with X25519, HKDF-SHA256
and ChaCha20-Poly1305. What a sealed value is (`About`: whose, what
kind, which version) is its additional data, so it opens only as what
it was sealed as.

### A recipient for one transfer

For a session, create a fresh `Recipient` and drop it when the transfer
ends. A captured stream cannot be opened after its private key is gone.
`keep` and `from_kept` also support durable recipients, such as a machine
receiving actions while offline; retaining that key retains the ability
to open earlier ciphertext. A sealed stream (`stream`) seals a fresh ChaCha20-Poly1305
secret in its header; numbered records authenticate their length and
order, including a required empty final record, and a failed writer
never makes a successful end.

### What none of them does

None prints what it holds: `Debug` writes `<redacted>`, so a log line,
an error or a struct printed with `{:?}` carries none. None has
`Display`: what takes one would send `<redacted>` as the value, so it
doesn't compile, and the value is exposed there, by name. `Secret` zeroes
its held value on drop; byte secrets and HMAC tags compare in constant
time (`subtle`).

#### Key Material in Memory

`SigningSecret` keeps its PKCS #8 bytes in a `Secret`,
but each signing or public-key operation creates a temporary `ring` key
pair whose internal memory is not zeroed on drop. These types cannot
erase copies made before wrapping a value or after explicitly exposing it.

## `hmac`

Shared HMAC-SHA256 signing and verification on `ring`, with one hex
encoding for HMAC signatures such as agent proofs and signed webhooks.
Ed25519 uses the separate `signing` module above.

## `inbound`

Every post from outside that a secret vouches for (a connection's webhook,
a payment's, a repository's, an agent's own) is checked here, built
once. A receiver says how its sender signs, as a value (`Scheme`); it
never writes verification.

### What `Inbound::verify` gives every receiver

The supplied body slice is checked against `max_body` before signature
processing (`Refused::TooBig`). The HTTP receiver must separately bound
its read before buffering that slice. Closed by default, a receiver
without a secret refuses every post
(`Refused::Unconfigured`), and nothing builds one that takes anyone; a
constant-time compare, of the secret or of its HMAC. A scheme declaring
a signed time checks its window (`Refused::Stale`); a header-secret
scheme has no timestamp or delivery ID.

### Replays, rolling and answers

The delivery's id, for a `Seen` store to refuse a replay
(`Inbound::take`); both the current secret and the one before it
tried, so any secret rolls with no restart (`Secrets`); and one answer
for each refusal (`Refused::status`).

## `process`

A process safe to hold secrets, and its children given none but on
purpose.

### `guard`

Call first, before reading secrets or starting threads. Check the returned
`Guarded`: protection can fail. On Linux, `undumpable` records whether
`PR_SET_DUMPABLE` succeeded, restricting same-user access through
`/proc/<pid>`; the core-file soft limit is also set to zero where possible.
On macOS it records `PT_DENY_ATTACH`, with the same core-limit attempt.
Other systems return false. `memory_locked` is Linux-only and requires
an unlimited lock limit or `CAP_IPC_LOCK`; no limit is raised. Debug builds
can disable the guard with `LNK_SECRET_DUMPABLE` for debugger tests.

### `Secret::from_env`

Marks the variable it read as taken, and takes it out of the
environment while the process has one thread, since changing the
environment with another thread running isn't safe.

### `command` and `ours`

`command` starts every child: its environment the parent's but every
variable taken and every one whose name says it's a secret's
(`names_a_secret`). `ours` starts a child that's one of Link's own
programs: the same, and every secret this one holds handed to it on a
pipe, where its `Secret::from_env` finds them, so a key you set for
`lnk` reaches the module that reads it.

### The pipe

It goes to that child alone: every child started here closes it, so it
reaches no program the child starts in turn, but a program of its own
not built on this package (a module from outside Link) gets it as
Link's own do.

### `ends_with_parent`, `stand_in_front` and `carried`

On Linux, `ends_with_parent` sets the child's parent-death signal; on
other systems it adds no such behavior and the caller must own cleanup.
`stand_in_front` runs a child this process waits for, its
stop passed on to it; and `carried`, a program run for the one that
started this one, as a wrapper runs it, with the environment it was
given for it, whole.

### A child that passes on no stop

On Linux, `pass_to_group_led_by` sends the stops `stand_in_front` passes
instead to the process group a pidfd's process leads, while it has a
member, for a child such as bubblewrap that passes on no signal itself.
`wait_in_front` waits for the child and stops passing stops on before
reaping it, so no stop reaches a process number it no longer holds.

### `watched_by_parent`

`watched_by_parent` ends a child that never looks for its parent, such
as a third-party server, on every system: when this process ends,
however it ends, the child and its process group get its signal. A
watcher, forked in the child's place before its program runs, keeps
only the read end of a pipe whose write end this process alone holds,
and signals the child when the pipe closes, then its group. It leaves
once the child ends, and it's in the child's group, so stopping the
group stops it; it signals the child first, so a child that left the
group is still reached.

### `give`

The one way to hand a child a secret in its environment, for a program
that reads one nowhere else: each a reviewed place, with why.

### `OwnedChild`

`process::OwnedChild` owns a subprocess and its process group on Unix.
It observes exit without reaping the leader, stops the group before
reaping and refuses numeric group signals if another reaper has taken
ownership. Its child borrow is for pipes and identity only, never
`wait` or `try_wait`. `release` transfers the complete cleanup obligation
to a separate owner, as a durable service controller does. Descendants
must remain in the owned group for group cleanup to cover them.

## Derive Explicit Copies

[Secretive Derive](../../../../derive/docs/more/libraries/files/secretive/derive.md) documents the derive macro for types that hold secrets,
including how to distinguish secret fields from fields copied through `Clone`.
