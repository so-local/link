# Signer

`link-signer`: a signer and its verifier, Ed25519 signatures, the compact
JWS every token Link mints is, and the short signed statement a principal
trades for a token.

<!-- docs-tree:start -->

## Docs

- [Signer](docs/more/libraries/files/signer.md): `link-signer` signs with a private half that never leaves its machine and checks those signatures anywhere with the public half

<!-- docs-tree:end -->

## Code

- [`jws.rs`](src/jws.rs): a compact JWS, made and read in two steps
- [`signature.rs`](src/signature.rs): the statement spent once
- [`signer.rs`](src/signer.rs): the private half
- [`verifier.rs`](src/verifier.rs): the public half and its fingerprint
