# Security

> Security You Can See and Check

Keep every program Local Link runs to the folders and hosts
you allow, and check that the limits hold.

<!-- quickstart -->

## Run a Program in a Sandbox

Give a program only this folder, then see what else it can reach. The
[sandbox](../../sandbox/docs/security/sandbox/README.md)
asks you first when a program wants more, and says where it can't run.

```sh
lnk sandbox check --ready
lnk sandbox run --commands.toolchains shell --files.write . -- bash
```
