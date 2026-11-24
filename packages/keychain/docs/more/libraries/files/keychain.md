# Keychain

`link-keychain` saves secrets in the macOS Keychain through Apple's
`security` tool on a Mac, and in private files on other machines.

## Saving and loading

`get`, `set` and `delete` manage accounts under the `link` service.
Values reach the tool on standard input, never in command arguments.
`Kept` pairs an account with the file that keeps it where there is no
Keychain. `stash` and `fetch` let callers save and load credentials
without changing their in-memory representation. Callers retain
ownership of credential policy.

## Reading a secret back

`get` reads with `security find-generic-password -g`, not `-w`. `-w`
prints a secret holding any unprintable byte, such as the newlines of
an SSH private key, as bare hex, indistinguishable from a secret that
is itself hex. `-g` writes `password: ` followed by either `0x` and the
bytes in hex or the text in quotes, so every secret reads back as it
was saved.

## Failing closed

Every save and load returns an error rather than a fallback. A Keychain
that refuses a secret, or can't be read because it is locked or asks for
a person, fails the command. On a Mac a settings file holding a secret
the Keychain keeps is refused, never read, so no plaintext copy is used.

### A Keychain That Asks

`security` answers in well under a second unless it is showing a person
a dialog, so every call has a limit (`ANSWERS_WITHIN`, in the crate's
`constants.rs`). Past it, `security` is ended and the command fails,
saying the Keychain asked for a person.

### No Default Keychain

A save first asks `security default-keychain`. A user with no default
keychain, such as a process given a `HOME` that isn't its user's, would
see `security` ask where to keep the item, so the save is refused at
once, naming that cause.

## Availability

The store is chosen once per platform. Keychain storage is always on
for macOS, and elsewhere callers keep secrets in private settings files
(mode 600). In a debug build alone, `LNK_SECURITY` selects another
`security` executable for isolated tests and turns the Keychain on
anywhere; a release build always runs `/usr/bin/security`, so no
variable set where `lnk` starts can send a secret to another program. The testing kit's
stand-in, [`link_test_kit::keychain`](../../../../../test-kit/src/keychain.rs),
is that executable.
