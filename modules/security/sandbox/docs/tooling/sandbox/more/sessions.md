# Sandboxed SSH Sessions

On a box, a person gets a Linux user of their own, whose every SSH
session is in their sandbox: [A box's
users](../../../../../../compute/box/docs/services/boxes/user/README.md#add-a-user).
What their key runs is `lnk sandbox enter`, which any key's forced
command can run.

```text
restrict,pty,command="/usr/local/lib/lnk/lnk-sandbox enter --spec /etc/lnk/users/alice/alice.toml --folder /home/alice" ssh-ed25519 AAAA...
```

## Session Commands

A shell, a command, `scp` or `sftp` (the session's
`SSH_ORIGINAL_COMMAND`) all run in the sandbox the specification makes, named for
its file, with the folder the one place it writes. Its `[commands]` must
let `/bin/bash` run, which a session is.

## Inspect a Session

`lnk sandbox enter --check` says what the sandbox opens, a line each,
and runs nothing; a command after `--` runs in place of the session's.

