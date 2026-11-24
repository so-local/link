# Box Users

> Users Share a Box, Never Each Other's Files

Give people Linux logins on a box, each confined by a sandbox specification. Run
these commands in a shell on the box, opened with
[`lnk box ssh`](../ssh.md#open-a-shell-on-a-box), as root, with `sudo`
before each. Before you add a login, copy the person's public SSH key and
a sandbox file to the box with `lnk box copy`. You reach a box yourself with
Local Link's own SSH key, which [Box Key](key/README.md) prints.

## Add a User

`lnk box user add` creates the user's home and limits their SSH key to
entering the sandbox. Shells, remote commands, and file transfers all run
in that sandbox. Its command policy must permit `/bin/bash`. Adding the same
user again replaces their key and sandbox.

```sh
lnk box user add alice --public-key alice.pub --sandbox dev.toml
```

## List Users

`lnk box user list` shows the users that Local Link manages on this machine.
It refuses inside a box here. A box here has a folder, not a separate
machine's system users.

```sh
lnk box user list
```

## Remove a User

`lnk box user remove` removes the login and its SSH access. The home folder
stays unless you pass `--delete-folder`, so copy any files you need first.
A folder that stays becomes root's alone, because the next user you add may
get the removed user's ID.

```sh
lnk box user remove alice
```

<!-- docs-tree:start -->

## Docs

- [Box Key](key/README.md): Local Link lets you into every box with one SSH (Secure Shell) key of its own

<!-- docs-tree:end -->
