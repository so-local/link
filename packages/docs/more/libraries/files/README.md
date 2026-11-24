# Files and Secrets

> Read State Through Its Owning Boundary

[Files](../../../../files/docs/more/libraries/files/files.md) supplies filesystem
and location primitives. [Settings](../../../../settings/docs/more/libraries/files/settings.md)
uses those boundaries to load application settings and control files.
Choose the owning API instead of assembling a second path or trust reader.

## Content and Secrets

[Artifact](../../../../artifact/docs/more/libraries/files/artifact.md) fetches
verified immutable content, and [Digest](../../../../digest/docs/more/libraries/files/digest.md)
defines the identity of that content. [Secretive](../../../../secretive/docs/more/libraries/files/secretive.md)
keeps secret values out of ordinary diagnostics.
[Keychain](../../../../keychain/docs/more/libraries/files/keychain.md)
connects callers to the operating system's store for secrets.

## Signing and Tokens

[Signer](../../../../signer/docs/more/libraries/files/signer.md) signs with a
private half that never leaves its machine and checks the compact JWS
every token Link mints is. [Token](../../../../token/docs/more/libraries/files/token.md)
builds Link's one token on it, with the service credential traded for
one and the single match of an action against its scope.
