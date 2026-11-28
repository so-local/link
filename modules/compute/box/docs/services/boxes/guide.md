# Boxes Guide

**A computer in your own cloud, running `lnk`.** Connect your AWS or
Google Cloud account once, and one command makes a machine there with
`lnk` on it.

```sh
lnk up
lnk cloud connect aws
lnk box create aws
lnk box ssh aws-1
lnk box list
lnk box stop aws-1
lnk box start aws-1
lnk box delete aws-1
```

## What it needs

You need a Mac or Linux with OpenSSH, and your own AWS or Google Cloud
account. [`lnk up`](../../../../../../docs/more/modules/install.md#pick-your-modules) adds the modules, and [`lnk cloud connect`](../../../../../cloud/cloud/docs/services/cloud/guide.md#connect-a-cloud-account) offers
to install the cloud's CLI.

### Boxes here

`lnk box create here` makes a box on this machine, with no cloud
account. A box here is a folder of its own,
`~/Link/Boxes/<name>/home`, which its commands start from. Box keeps its
record beside your cloud boxes in `boxes.toml`.

```sh
lnk box create here --name here-1
lnk box ssh here-1
```

Isolation is a separate choice, made with the same flags as
[`lnk agent create`](../../../../../agents/agent/docs/agents/start/create.md#create-on-a-hosting-account):

| Configuration | Flags | What runs the box's commands |
|---|---|---|
| Sandbox, the default | none, or `--sandbox <spec>` | This machine, inside the box's sandbox |
| Directly | `--no-sandbox` | This machine, with your own access |

#### Sandboxed or direct

A sandbox here is [`lnk sandbox run`](../../../../../security/sandbox/docs/tooling/sandbox/guide.md#sandbox-guide) with the [sandbox specification](../../../../../security/sandbox/docs/config/specs/sandbox.md)
given, read once when the box is made, or Link's own, and
[Security](security.md#boxes-here) says what each choice lets in.

#### Its commands

`lnk box run`, `ssh` and `copy` act in the home. `start` and `stop`
start and stop its environment's services. `delete` stops them and
removes the record, and removes the home only once you confirm.
`lnk box user` is refused inside a box here, since a box user is a login
on a machine of its own.

### Machine inputs

`--env` names an environment that sets the box up, on any placement,
as [Use an Environment on a Box](lifecycle/README.md#use-an-environment-on-a-box)
explains. A box here runs its stages in their sandboxes, or directly
with `--no-sandbox`.

### What it costs

Boxes in a cloud account cost money, billed to your account by your
cloud, as [Box Costs](lifecycle/README.md#box-costs) explains. Local
Link adds no charge for them.

## Connect a cloud account

A box is made in a cloud account you connect once, which keeps your
buckets too: [Platforms](../../../../../cloud/cloud/docs/services/cloud/README.md) has connecting and
disconnecting one.

## Create a box

A box costs money, so `create` shows the machine it picked, its disk and
the price when the cloud gives one, then asks.

```sh
lnk box create aws
lnk box create gcp --zone europe-west1-b --machine.gcp.size e2-standard-4
lnk box create aws --machine.cpus 4 --machine.memory.min "8 GB" --machine.memory.max "16 GB"
```

A new box is Ubuntu 24.04 on x86_64 with a 20 GB disk and only SSH
open. It gets `lnk`, and what you run on it adds its own modules. `lnk
agent move aws` makes a box itself when you have none in `aws`.

### Run Your Own Build

`--bundle` makes the box from a bundle of your own checkout. Give it the
name of one [`lnk bundle`](../../../../../tooling/bundle/docs/tooling/bundle/README.md#bundle)
kept, such as `devbox`, which means the newest `devbox` it made, or the
bundle's folder by a path, wherever it is. A word alone is always a
kept name, so a folder of the same name here is `./devbox`. That
bundle's own `lnk`, which trusts the signer you made it with, makes the
box and installs the bundle on it. [Run from the bundle's `bin/`](../../../../../../scripts/docs/more/developing/releases.md#a-bundles-own-programs), its `lnk`
makes every box from the bundle without `--bundle`.

```sh
lnk box create gcp --name gcp-1 --bundle devbox
lnk box create gcp --name gcp-2 --bundle ~/builds/devbox
```

The box keeps the copy of the bundle it was given, and installs every
later module from it. A module that `lnk box run`, [`lnk agent move`](../../../../../agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud),
or a command typed on the box itself, such as [`lnk agent tool add`](../../../../../agents/agent/docs/agents/README.md#tools), adds
there comes from your bundle, never Link's public releases, which hold
no build of your checkout.

## Reach a box

`lnk box ssh` opens a shell on a box or runs one command there,
`lnk box run` runs `lnk` there, `lnk box copy` copies files to it, and
`lnk box forward` sends its traffic out through this computer. All four
use one SSH connection, with Local Link's own key and certificates, and
[Reach a Box](ssh.md) explains each command, its credentials, what it
needs and how it fails. Other people get logins of their own, each in a
sandbox, with [Box Users](user/README.md).

## Set it up your way

Describe the machine in a file, set it up with a script, and save the
result as an image, and the next box is one command away.

### Describe a box in a file

A box specification says the machine in a file, so the same file makes the same
box, on any cloud you've connected.

```sh
lnk box size list aws --spec web.toml
lnk box create aws --spec web.toml
lnk box create gcp --spec web.toml --machine.spot
lnk box spec show --machine.cpus 4 --machine.disk.min 40GB
```

A box specification is a TOML file that describes the machine the way you would
at a store. Save it as `web.toml`, in any folder, and give its path to
`--spec`. Its `name` names its boxes, so this one's are `web-1`, `web-2`
and so on.

```toml
# web.toml
schema = 1
name = "web"

[machine]
cpus = { min = 2, max = 8 }
memory = { min = "4 GB", max = "16 GB" }
disk = { min = "40 GB" }

[machine.aws]
region = "eu-west-2"
```

The box gets the smallest machine that fits. The `min` is what it needs
and `max` the most you'll pay for, and the same file makes the best fit
on AWS and on Google Cloud. A flag wins over the file. Every field is in
the [box specification](../../config/specs/box.md).

#### The specification a box was made from

`lnk box spec show` prints the specification a box was made from, with
its machine pinned for its cloud. Save what it prints as a file, such as
`web-1.toml`, and that file makes exactly that box there, as
[what it prints](../../config/specs/guides/box.md#what-lnk-box-spec-show-prints) explains.

```sh
lnk box spec show web-1
```

### Set a box up in stages

A box is set up in stages: each stage one command, `lnk box copy`
putting its files on the box, and its image saved once by a hash of what
made it. An environment file declares the same stages, and
`lnk box create --env` runs them for you, building and reusing the image
by its hash, as [Use an Environment on a Box](lifecycle/README.md#use-an-environment-on-a-box) explains.

#### Sandboxed Stages

Run each stage that fetches or builds in a sandbox of its own on the box,
so cloning reaches only GitHub and installing only the package registry.
The sandbox costs nothing beyond the box. These stages install pip,
ready the box's sandbox, copy the two sandbox specifications into
`~/setup` on the box, then clone the app and install its dependencies.

```sh
lnk box ssh web-1 -- sudo apt-get install -y python3-pip
lnk box run web-1 -- sandbox check --ready --yes
lnk box copy web-1 clone.toml deps.toml setup
lnk box run web-1 -- sandbox run --spec setup/clone.toml --yes -- git clone https://github.com/me/app app
lnk box run web-1 -- sandbox run --spec setup/deps.toml --yes -- python3 -m pip install --no-cache-dir --target app/.deps -r app/requirements.txt
```

`clone.toml` sits beside your other setup files on this computer. Its
sandbox reaches GitHub alone, runs only `git`, and writes `~/app` with
its `.git`.

```toml
# clone.toml
schema = 1
name = "clone"
version = 1

[files]
write = ["~/app"]
write_dot_files = ["~/app"]

[internet]
hosts = ["github.com"]

[commands]
allow = ["git", "/usr/lib/git-core/"]
```

`deps.toml` is the same with no `write_dot_files`, reaching only
`pypi.org` and `files.pythonhosted.org`, and allowing only `python3`. A
sandbox specification's sections are in the
[sandbox](../../../../../security/sandbox/docs/tooling/sandbox/README.md) docs.

#### A stage that fails

A stage that fails exits with its own status, and the stages before it
have run. Fix the command and run the rest, or start again on a new box.
Kept in a script of your own, the stages run again as one, and the first
can make the box itself with `lnk box create aws --name web-1 --yes`.

#### Folders a stage writes

What a stage's sandbox makes, and the dot files it keeps read only, are
in [What's on a Box](lifecycle/on-a-box.md#folders-a-stage-writes).

### Save a box as an image

An image is a box's disk, kept in your cloud account so new boxes can
start from it. Saving needs a running box with no agent and none of the
usual secrets on it, signs the box's `lnk` out first, and stops the box
while its disk is saved. [Saved Images](image/README.md) shows how to
create, list and delete an image, what each costs, and where a box made
from one is made.

```sh
lnk box image create web-1 web-ready
lnk box create aws --image web-ready --spec web.toml
```

### Reuse an image while what made it stays the same

An image saved with a hash of what made it is found again by that hash,
so a setup is saved once. The hash is yours to make, such as the first
characters of `shasum -a 256` over your setup's files and lockfile, so a
change to any of them makes a new hash. Ask for an image with the hash.
When none is listed, make a box, run the setup on it, save it with the
hash and delete it. Every later box starts from the newest ready image
with that hash.

```sh
lnk box image list aws --hash 3f2a9c
lnk box create aws --name build-1 --yes
lnk box image create build-1 app-3f2a9c --hash 3f2a9c --yes
lnk box delete build-1 --yes
lnk box create aws --name web-1 --image-hash 3f2a9c --yes
```

### Attach existing resources

A box specification's `[attach]` names an existing `network`,
`identity`, `address` and `disks`, checked before launch and kept at
removal, as the [box specification guide](../../config/specs/guides/box.md#attachments)
explains.

## See your boxes

Every box you made is found from any of your computers, with its
account, place, size, address and state.

### List Your Boxes

`lnk box list` shows every box tagged as yours, with its account, place,
size, address and state.

```sh
lnk box list
lnk box list --refresh
lnk box list --all-regions
lnk box list --here
lnk box list aws --tag project=research
```

`list` asks each connected account for the boxes tagged as yours, finding
those made on other computers, and `--refresh` asks each box's state now.
On AWS it looks in your boxes' regions, or every enabled region with
`--all-regions` or when it knows none, naming a region your login can't
reach. `--here` asks no cloud, and `--tag` finds boxes by tag across owners.

### Show One Box

`lnk box show` reads the saved record for one box, including its last observed
address, size, placement, and state. It asks no cloud, so use `lnk box list
--refresh` first when you need a fresh provider observation.

```sh
lnk box show aws-1
```

### Stop, Start, or Delete a Box

A box can be stopped and started any number of times, its disk kept and
billed, and is deleted with its disk at the end. [Box Life Cycle](lifecycle/README.md)
explains each stage, what happens on the box, and how a deletion is
confirmed.

## Get into your boxes from a new computer

Sign in to the same Link account and connect the same cloud account, and
the new computer finds your boxes by their tags. Its first connection
pins each box's host keys from the cloud and has the box trust this
computer's certificates too, as
[Reach a Box](ssh.md#first-connection-from-another-computer) explains.

## Where things live

Local Link keeps your cloud accounts in `~/.lnk/safe/clouds/accounts.toml`,
this computer's copy of your boxes in `~/.lnk/safe/boxes/boxes.toml`, and
its SSH key, certificate authority and pinned host keys beside them.
[Files](lifecycle/files.md) lists every file, on this computer, on each
box and in the cloud, and what each one holds.

## More about it

Every flag is in `lnk box <command> --help`. [Security](security.md) has
what Link guarantees and its gaps, [Decisions](decisions.md) why it works
this way, and [Troubleshooting](troubleshooting.md) what to do when a
box won't.
