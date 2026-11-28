# Troubleshooting

What to do when [`lnk` doesn't install](install.md), or [a module won't install or
update](README.md).

## Installing

The installer checks the release's signature before it puts `lnk` in
place, so most of what goes wrong is that check or the `PATH`.

### `lnk: command not found` After Installing

Add `~/.lnk/exec/bin` to your `PATH`: the installer leaves your shell's
settings alone, and prints the line to add.

### "you can't write to ..."

`lnk` installs its modules in its own folder, so it goes in a folder you
can write. Set `LNK_INSTALL_DIR` to one you own, or leave it out for
`~/.lnk/exec/bin`.

### "couldn't download ..."

The network didn't answer for the file it names. Check your connection
and run the install line again. "the release ... has no" names a file
the release lacks, as a version that doesn't exist would.

### "Your modules are another release's"

The installer replaced `lnk` and left the modules an older install put
beside it. Run `lnk upgrade` to bring them, and your files, to the new
release.

### "ssh-keygen (OpenSSH) is needed to check the release signature"

Install OpenSSH 8.1 or newer, or set `LNK_INSECURE=1` to install
without checking the signature.

### "the release signature did not verify"

Check that `ssh -V` is 8.1 or newer. If it is, don't install, and [report it as a vulnerability](../vulnerabilities.md).

### Link 0.0.0 Doesn't Upgrade

Link 0.0.0 is a pre-launch release, and no other release upgrades it.
Its copy on GitHub is replaced as it changes, so `lnk upgrade` never
sees a newer 0.0.0. It trusts a signer that no later release is signed
by, so `lnk upgrade` to a later version stops and says it doesn't
trust the release's key. No later release reads the files
0.0.0 wrote either. Install again with the install line to get the
latest release.

## Modules

A module installs when a command needs it, so most of what goes wrong
is a module `lnk` can't take, or one installed another way.

### A Command Says Its Module Isn't Installed

Run [`lnk mod add <module>`](guide.md#add-or-remove-a-module). Typed, `lnk` installs it itself: this
is a module's call underneath, or you said no.

### "a standalone program is not an installable module"

Only a module `lnk` runs is added from outside Link: its `lnk-<name>`
answers `--link-describe` as a module, never as a program of its own.

### A Module from Outside Link "not upgraded: ... doesn't trust"

Its release is signed by another key. If its maker says they changed
keys, [`lnk mod remove <name>`](guide.md#add-or-remove-a-module), then add it again.

### "The <module> module at ... isn't in lnk's folder"

`lnk mod remove` removes only what's in `lnk`'s folder, and leaves
one on your `PATH` to you: remove it the way you installed it.
