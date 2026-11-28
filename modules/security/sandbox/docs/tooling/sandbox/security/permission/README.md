# Sandbox Permissions

A sandbox opens nothing of this machine until its specification or a flag asks:
no files, no network, and no program beyond the system's own. Each thing
a sandbox can be opened to is a section of its [specification](../../../../config/specs/guides/sandbox.md),
and each key of a section is also a flag of `lnk sandbox run`.

## List the Permissions

`lnk sandbox permission list` prints each section of a specification with its
field flags and what it opens, then each toolchain `[commands]
toolchains` takes with the programs it stands for. The command reads only Local
Link's own declarations and changes nothing.

```sh
lnk sandbox permission list
lnk sandbox run --commands.toolchains shell --files.write . -- bash
```

The sections and flags that `permission list` prints are made from the
specification's one definition, as [Decisions](../../decisions.md#why-is-the-specifications-schema-made-from-the-rust-type)
explains, so they never drift from what a specification file accepts.
