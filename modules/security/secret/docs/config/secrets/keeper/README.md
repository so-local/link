# Secret Keeper

The keeper is a system user that keeps your secrets, and no program of
yours runs as that user. A program running as you therefore can't read
your secrets from disk. Making the keeper needs `sudo` once.

## Create the Keeper

`lnk secret keeper create` makes the keeper's user and moves what's
kept to that user. Your first `lnk secret create` on a Linux computer
asks once whether to make it, and `--yes` answers yes. It needs `sudo`
once, and systemd. Running the command again puts back what's missing.

```sh
lnk secret keeper create
```

### What `lnk secret keeper create` Does

The command makes the user `lnk-secrets`, which has no login and runs nothing
else. It copies this `lnk-secret` to `/usr/local/lib/lnk/lnk-secret`,
root's, which nobody else changes. Then it starts the service
`lnk-secrets`, which keeps each user's secrets in
`/var/lib/lnk/secret/<uid>/`, readable by it alone, and answers on
`/run/lnk-secrets/keeper.sock`. A caller there reaches its own user's
secrets alone, by its socket's peer user, and no sandbox has the socket.

#### Removed Users

Each user's folder notes the user's name at their first call, and is
theirs only while their ID still names them. A user removed leaves
their folder, and a user made later may be given the same ID. That
user is refused the folder, whose secrets were another's, until root
removes `/var/lib/lnk/secret/<uid>`. A user removed and made again
under the same name is taken for the same user.

### Answering No to the Keeper

No keeps your secrets in files of your own user, in
`~/.lnk/safe/secrets/secrets.toml` with mode 600, and says plainly that
any program you run outside a sandbox can read them. The answer is
remembered in `~/.lnk/safe/secrets/preferences.toml` (`keeper = false`),
so it isn't asked again, and `lnk secret keeper create` makes the
keeper later. With no terminal, nothing is asked: your secrets are
kept in your own file, and the command says so.

### Your Own File and Upgrades

The keeper never takes your own file. While the file holds a secret,
`lnk secret keeper create` is refused, and so is every command once
there's a keeper. Remove the file, then add your secrets again. [`lnk
upgrade`](../../../../../../../docs/more/modules/guide.md#roots-copies)
puts the new `lnk-secret` in the keeper's place, by running `lnk secret
keeper create` again.

## Who the Keeper Hands a Secret To

The keeper hands a secret only where a grant names it, and only to the
command that starts a run. What starts the run pushes the secret into the run as it starts, and no
run asks the keeper for one.

### Sealed Secrets

A sealed secret reaches the run's proxy as ciphertext, an encrypted
grant. The keeper also serves `/run/lnk-secrets/secret.sock`, where it
checks the proxy's actual UID, its current bridge proof and the grant's
identity before opening the grant and adding the secret to the request,
outside the work's user and sandbox.

#### Bound to One Proxy

Only root may bind a grant for a different proxy UID. A replaced,
unbound or revoked grant stops opening, and registering the same proof
again never restores a revoked one. Restarting the keeper drops its
recipient and every route grant, so proxies bind again.

### Plain Secrets

A plain secret handed to a run is that run's to read, by design. The
keeper keeps it from every other run and off the disk.

## Only as Strong as the Working User Is Weak

The split holds only where the user the work runs as has no `sudo`.
Packed units have separate users without sudo. Ordinary boxes give each
sandbox [a fresh locked host user and groups](../../../../../../compute/box/docs/services/boxes/lifecycle/on-a-box.md#working-users),
and controllers and proxies stay outside it.

### On Your Computer

On your computer the work runs as you. The keeper keeps your secrets
out of your files (a backup, a synced folder, a program reading
`~/.config`) and out of every sandbox, but a program you run outside a
sandbox can ask the keeper as you do.

## Keeper on a Box

A box made by [`lnk box create`](../../../../../../compute/box/docs/services/boxes/guide.md#create-a-box)
installs and starts the keeper during setup, unasked, together with the
[worker launcher](../../../../../../compute/box/docs/services/boxes/lifecycle/on-a-box.md#working-users).
Ordinary-box work runs as separate users without sudo, and its proxy and
the keeper stay outside that identity and its sandbox.

### Encrypted Disk

A box's disk is encrypted by its cloud: always on Google Cloud, and on
AWS, where it's an account's setting, `lnk box create` asks for it on
the box's disk and says so. A secret sealed with a key kept on the same
disk would add nothing, so the keeper seals its store to a TPM where
the box has one.

### TPM (Trusted Platform Module) Sealing

With `/dev/tpmrm0`, setup installs `tpm2-tools` and gives the keeper access
to that device's group. Where the device is root's alone, as on Google
Cloud's Ubuntu image, setup first makes the group `lnk-tpm` and gives it
the device, now and at every boot (`/etc/udev/rules.d/70-lnk-tpm.rules`).
A device it can't give a non-root group is refused. Google Cloud's
Shielded VM and AWS's NitroTPM both provide one, and a box without one
says so when it's made.

#### What the TPM Seals

`tpm2-tools` creates a fixed-TPM, fixed-parent sealed recipient. Only
opaque TPM blobs and HPKE ciphertext persist, and TPM contexts live on
`/run`. Another TPM can't open a copied disk. Existing plaintext keeper
files migrate when they're read.

#### TPM Failure

A failing TPM is an error, and never falls back to plaintext. Without a
TPM the keeper keeps its private files. A person who declined the
keeper keeps the explicit mode 600 file even on a computer with a TPM.
