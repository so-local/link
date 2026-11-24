# Secretive

`link-secretive` holds every secret Local Link keeps, in one of three types by where it may go: opaque, exposed only by name, or sealed to one recipient. It never prints a secret, zeroes it when dropped, checks every signed post from outside, and guards a process that holds secrets.

<!-- docs-tree:start -->

## Docs

### Pages

- [Secretive](docs/more/libraries/files/secretive.md): `link-secretive` holds every secret Link holds, in one of its types by where it may go, so a wrong way doesn't compile

### Packages

- [Secretive Derive](derive/README.md): `link-secretive-derive` provides `#[derive(Duplicate)]` for a type that holds a secret

<!-- docs-tree:end -->

## Code

- [`inbound.rs`](src/inbound.rs): checking a signed post from outside
- [`lib.rs`](src/lib.rs): the secret types
- [`process.rs`](src/process.rs): a process safe to hold secrets
- [`sealed.rs`](src/sealed.rs): a secret sealed to one recipient
