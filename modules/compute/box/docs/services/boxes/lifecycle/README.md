# Box Life Cycle

> Every Box From First Boot to Final Delete

A box lives from `lnk box create` until `lnk box delete`, and this page
follows it through each stage. It covers what Local Link makes, what
runs and is installed on the box, the files and records it keeps, and
how a deleted box is confirmed gone.

## Stages of a Box

A cloud box is made running, can be stopped and started any number of
times, and ends when it's deleted, its disk kept through every stop. A
box here follows the same stages with a folder in place of a machine.

### States You See

`lnk box list` shows each box's state as its cloud or Local Link last
saw it, one of these.

- **Running:** the machine is up and takes SSH.
- **Pending, starting, stopping:** the cloud is changing the machine's state.
- **Stopped:** you stopped it, its disk kept and billed.
- **Stopped by its cloud:** the cloud took a spot machine back, its disk kept.
- **Stopped elsewhere:** found stopped on Google Cloud, by its cloud or by another computer.
- **Terminated, gone:** the cloud no longer has the machine.

## Create a Box

[`lnk box create`](../guide.md#create-a-box) makes the box, and asks before
anything is billed. The steps below happen in order. When the cloud
can't make the machine, Local Link removes what it made for boxes there.
A machine made but not set up stays, and
[Troubleshooting](../troubleshooting.md#made--but-couldnt-set-it-up) says
what to do with it.

### What Creation Does

Local Link first checks that `ssh` and `ssh-keygen` are installed, reads
the box specification and every file it names, and picks the smallest
machine that fits. It checks the `lnk` release the box will install
against this computer's own signing keys, then asks you, showing the
machine, its disk and its price. Once you agree, it keeps the box's name
in its records and asks the cloud for the machine, tagged as yours.

### First Boot

The machine's first boot (cloud-init, Ubuntu's first-boot setup, run as
root) writes `/etc/lnk/kind.toml` to say the machine is a box, tells its
SSH server to trust this computer's certificates, and prints its host
keys. Local Link reads those keys from the machine's console and pins
them, waits for SSH, then installs `lnk` with the Secret and Sandbox
modules and signs the box in as you. [What's on a Box](on-a-box.md)
explains each of these.

### Creating a Box Here

`lnk box create here` makes no machine. It makes a home folder,
`~/Link/Boxes/<name>/home`, captures the box's sandbox specification,
and keeps a record in `~/.lnk/safe/boxes/boxes.toml`, as
[Boxes Here](../guide.md#boxes-here) describes.

## Use an Environment on a Box

A box needs no environment, and a plain box is Ubuntu with `lnk` on it.
An environment is a file, `env.toml`, that says what a machine has
before work starts: the values every run is given, the stages that set
it up, and the services it runs. `lnk box create` names one with `--env`
or the box specification's `env`, so the box is set up the same way each
time. [Environments](../../../../../env/docs/services/environments/README.md)
explain the file and its commands.

```sh
lnk box create aws --env env.toml
```

### Environment Files a Box Keeps

The box captures the environment file and the files it names once, when
it's made, and never reads them from your computer again. Its image
stages run once, on a box made only to build the image, and the rest of
its stages and its services run on the box, again at each `lnk box
start`. [Environment Life Cycle](../../../../../env/docs/services/environments/lifecycle.md#put-an-environment-on-a-machine)
follows the environment's side of this.

## While a Box Runs

A running box is a computer of yours that Local Link set up and then
left alone. Nothing of Local Link's runs on it unless you, or a program
you started, asked for it.

### Programs Running on a Box

Ubuntu's SSH server lets this computer in, the first boot turns on
lingering so the `ubuntu` user's services keep running with no one
logged in, and an environment's services run as background services.
Everything else runs because you started it, with
[`lnk box ssh`](../ssh.md#open-a-shell-on-a-box) or
[`lnk box run`](../ssh.md#run-lnk-on-a-box).

### What Is Installed

A new box is Ubuntu 24.04 on x86_64 with a 20 GB disk, unless its
specification asks for another release, processor or disk. It holds
`lnk` with the Secret and Sandbox modules, in `~/.lnk/exec`, and every
module a later command adds there comes from the same releases. A box
made from an image starts with that image's disk instead, as
[Box Images](../image/images.md#a-box-from-an-image) describes.

### Files on a Box

What you write on a box stays on its disk, through every stop, until the
box is deleted. The cloud keeps the record of each box by its tags, and
this computer keeps a copy in `~/.lnk/safe/boxes/boxes.toml`, which
`lnk box list` updates from each cloud and `lnk box show` reads alone.
[Files](files.md) lists every file Local Link keeps, here, on the box
and in the cloud.

### Box Costs

A running box bills your own cloud account for its machine and its disk,
and a stopped one for its disk alone. An image you save bills for its
storage until you delete it.

## Stop a Box

`lnk box stop` stops the machine and keeps its disk, so nothing on the
box is lost. A stopped box still bills for its disk. A box here stops
its environment's services, and its home stays as it is.

```sh
lnk box stop aws-1
```

## Start a Box

`lnk box start` starts a stopped box again. The box usually comes back
at a new address, and Local Link checks it against the same pinned host
keys, waits up to 5 minutes for it to take SSH, then starts its
environment again. A new box is `lnk box create`, never `start`.

```sh
lnk box start aws-1
```

### Stopped by the Cloud

A spot box is cheaper, and its cloud may take the machine back. The
cloud stops it, never deletes it, so its disk stays and is billed, and
`lnk box list` says the cloud stopped it. On AWS the box starts again by
itself once AWS has room. On Google Cloud, `lnk box start` starts it,
as the [box specification guide](../../../config/specs/guides/box.md#spot)
explains.

### Deadlines

A box made with a deadline, such as `deadline = "4h"` in its
specification or `--deadline 4h`, ends itself then, at most seven days
after it was made, as the [box specification guide](../../../config/specs/guides/box.md#deadline)
describes.

## Delete a Box

`lnk box delete` deletes the machine and its disk, after asking. What
Local Link made for boxes in that place goes once no box uses it, your
images and attached resources stay, and `lnk box list --refresh` shows
the box gone. [Delete a Box](delete.md) explains each of these, and how
to delete a machine by its cloud ID.

## What a Box Holds

[What's on a Box](on-a-box.md) explains how a box installs `lnk`, how
its working users and the people you let in are kept apart, and how a
specification and a setup reach it. [Files](files.md) lists every file
[`lnk cloud`](../../../../../../cloud/cloud/docs/services/cloud/README.md#platforms) and `lnk box` keep, on this computer, on each box and in the
cloud, and what each one holds.

<!-- docs-tree:start -->

## Docs

- [Files](files.md): The files `lnk cloud` and `lnk box` keep, and what each one holds
- [What's on a Box](on-a-box.md): What a box holds, how it installs `lnk`, how its working users and its people are kept apart, and how a specification and a setup reach it
- [Delete a Box](delete.md): `lnk box delete` ends a box for good, deleting its machine and its disk after asking

<!-- docs-tree:end -->
