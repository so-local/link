# Interface

How a program is handed the secrets it may use, never through its environment.

## Secrets a program is handed

Link starts none of its programs [with a secret in its environment](../README.md#no-child-carries-one-it-wasnt-given). A module, an adapter or a
tool is handed the secrets the program that started it holds (those
you set for `lnk`, a connection's token, a model's key) on a pipe instead:
`LNK_SECRETS_FD` names the descriptor, 60, and it holds each as its
variable's name and its value, each ended by a zero byte, up to 16 KiB.

### Reading it

Read it once, first, and close it: `link_secretive::Secret::from_env`
does, when its program calls `link_secretive::process::guard` at
the start of `main`. A program written without `link_core` reads the
same; a shell script, from `/dev/fd/60`. Every child a program built on
`link_core` starts closes the pipe, so it reaches that program alone,
and more than a pipe holds unread fails the start rather than holding
it up.

## Remove a Remote Copy

Move adapters use `lnk secret delete --here --service service:<id> --own <use>`
through [`lnk box run`](../../../../../../compute/box/docs/services/boxes/ssh.md#run-lnk-on-a-box) to remove one agent-owned copy. The stable service
and use select that credential without affecting another agent or a
new box that reused its name. Public deletion coordinates this through
the `secret.copies` question and retains unreachable places for retry.

## Isolated Bootstrap Tests

The bootstrap fixtures replace privileged machine resources with owned
stand-ins. Release builds ignore these debug inputs.

### `LNK_TEST_BOOTSTRAP`

Permit the isolated bootstrap launcher to receive box credentials without
root. This tests the launch protocol without installing a system keeper.

### `LNK_TEST_TPM_DEVICE`

Select the test TPM (Trusted Platform Module) device path instead of
`/dev/tpmrm0`. This changes the debug fixture's device selection, rather than
proving that the real machine can seal and recover a secret.
