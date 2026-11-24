# Confinement

The package `link-sandbox-confine` confines a program as tightly as the
operating system allows while the program can still run. On Linux it uses
bubblewrap, Landlock, and a seccomp filter. On macOS it uses a Seatbelt
profile. Both sit behind one `Backend`, and only `lnk-sandbox` links the
package.

<!-- docs-tree:start -->

## Docs

### Pages

- [Sandbox Isolation](docs/security/sandbox/wall/README.md): The sandbox confines each program as tightly as the operating system allows while the program can still run

### More

- [Decisions](docs/security/sandbox/wall/decisions.md): Why the sandbox's wall, `link-sandbox-confine`, works the way it does

<!-- docs-tree:end -->

## Code

- [`lib.rs`](src/lib.rs): the backend
- [`linux/mod.rs`](src/linux/mod.rs): the Linux wall
- [`macos/mod.rs`](src/macos/mod.rs): the macOS wall
- [`rules.rs`](src/rules.rs): a confinement resolved on this machine
