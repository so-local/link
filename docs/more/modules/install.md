# Install

Install `lnk`, then pick the parts of Local Link you want. `lnk` is one
small command with a module for each part, so you install only what you
use.

```sh
curl -fsSL https://local.link/install.sh | sh
lnk up
lnk upgrade
```

## What You Need

You need macOS on Apple Silicon or Intel, or Linux on x86_64 or arm64,
and OpenSSH 8.1 or newer, which macOS and most Linux have already. It's
free, and installing needs no account.

## Install

The installer puts `lnk` in `~/.lnk/exec/bin` after checking the release's
signature with `ssh-keygen`. It leaves your shell's settings alone and
prints the line for your shell to add if that folder isn't on your
`PATH`. To choose a version, set `LNK_VERSION` on the install line:
`curl -fsSL https://local.link/install.sh | LNK_VERSION=0.0.1 sh`.

### Another Folder

`LNK_INSTALL_DIR` chooses another folder, one you can write, since `lnk`
installs its modules beside itself; the installer says so before it
downloads anything if you can't. Installed over an older `lnk`, it says
which modules are the older release's, and `lnk upgrade` brings them
and your files up to date.
[Files and Variables](../../../core/cli/docs/more/architecture/files.md#installsh) lists every
variable, and [Install `lnk`](../developing/setup/install.md) covers
installing from source, or switching between a release and a source
build.

## Pick Your Modules

Run `lnk up` and choose the parts you need when prompted. It installs
their modules; you can also add an individual module with [`lnk mod add`](guide.md#add-or-remove-a-module).

```sh
lnk up
lnk mod add slack
```

| Choice | What it's for |
|---|---|
| [agents](../../../modules/agents/agent/docs/agents/README.md) | Your own AI, running here in a sandbox |
| [models](../../../modules/agents/agent/docs/agents/models/agent/README.md) | The models on this computer, as an API |
| [buckets](../../../modules/storage/storage/docs/services/storage/README.md) | Your files, backed up to your clouds |
| [boxes](../../../modules/compute/box/docs/services/boxes/README.md) | Machines in your own AWS or Google Cloud account |

`lnk up` takes choices only; a module's name, or a module from outside
Link, is `lnk mod add`'s. Only `lnk up` asks when you name none.
Run it again any time to add more. If you run a command whose module isn't
installed, `lnk` offers to install it. The [Modules Guide](guide.md#what-each-choice-installs)
lists which modules each choice installs, and how to add, update and
remove them.

## More About It

What to do when installing fails is in
[Troubleshooting](troubleshooting.md#installing), and what's checked
before anything installs is in [Security](security.md).
