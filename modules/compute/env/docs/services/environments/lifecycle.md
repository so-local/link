# Environment Life Cycle

An environment is a file you keep, `env.toml`, and Local Link keeps a few
records about it. This page explains what an environment is as a thing,
how it's created and changed, what happens when it's put on a machine,
how it's removed, and where each record lives.

## What an Environment Is

An environment is the setup every machine made from it receives: the
values every run is given, the stages that set the machine up, and the
services it runs. It isn't a running program and it isn't a machine. It
is a description, kept in your repository beside the code it sets up, and
read again by every command you give it to.

### Parts of an Environment

Local Link knows an environment by three things. The file is yours, and
only you change it. Its ID, `env:<id>`, is minted the first time it's
needed, such as when a box is made from it or a secret is granted to it,
and kept by the file's real path, so a copy or a move of the file is
another environment. Its image, when it saves stages in one, is a box's
disk in your cloud account, found by a hash of what builds it.

### Where an Environment Runs

An environment runs on a box, a machine in your cloud account that
[Boxes](../../../../box/docs/services/boxes/README.md) explain, or on the
computer you type on. Only a box runs the stages that need root and
builds an image. Your computer runs the stages that each run in a
sandbox, and refuses a root stage before anything runs.

## Environments and Docker

Docker runs programs in containers. A container is a process that the
operating system gives its own view of files, network and process list,
started from a Docker image, which packs a program with every file it
needs. Many developers describe their setup in a `Dockerfile`
and run it in containers on any machine with Docker installed.

### How Environments and Containers Differ

An environment sets up the machine itself rather than a container on it.
Its image is a whole computer's disk that a cloud boots, never a Docker
image, and its stages run on the machine as its own users, each in Local
Link's sandbox, which limits the files, network and commands that stage
reaches. So what you SSH into, what runs the tests, and what the services
run on are one machine, set up one way, with nothing between them.

### Running Docker in an Environment

An environment can still use Docker. A stage running as `root` can
install it, and a service running as `root` can run containers, on a box
of its own only. The sandbox refuses Docker's socket, since whatever
reaches that socket runs outside the sandbox, so a service that needs
Docker is never sandboxed, as [Decisions](decisions.md#why-do-services-run-in-docker-only-on-a-box-of-their-own)
explains.

## Create an Environment

An environment is created by writing its file: no command makes one.
Start from the smallest file, check it, and add the stages and services
your work needs, as the [Environments Guide](guide.md#environment-file)
shows. Each check reads the file strictly and names the line of
anything wrong, so a file that passes is one every command reads.

### Approve Secrets

A stage that needs a private value, such as a package registry's token,
names a secret, and you say yes once to giving it to that file.
[Environment Approvals](approval/README.md) keep that yes, tied to the
file's path, its name and the secret.

## Put an Environment on a Machine

An environment reaches a machine in one of three ways, and each reads the
same file the same way. `lnk env create` makes a new box from it. `lnk
box create --env` names it while making a box with a box specification
of yours. `lnk env setup` sets up a box you already have, or this
computer, without saving an image.

### What a Machine Keeps

A box captures the file and every file it names once, when it's made,
and keeps that copy. It never reads your copy again, so editing the file
changes no box already made. The box's environment services outlive the
command that started them, run as background services of the box, and
come back when the box starts, when its start stages run again from the
captured copy. [Box Life Cycle](../../../../box/docs/services/boxes/lifecycle/README.md#use-an-environment-on-a-box)
follows this from the box's side.

### Reset Between Runs

A service that keeps state, such as a database, can be put back as the
image had it with its own reset command, for a clean run without a new
box, as [Reset a Service](guide.md#reset-a-service) shows.

## Change an Environment

Edit the file. A box made after the edit gets the new file, and its
image is built again only when the edit changes what builds the image,
such as a stage's command or a lockfile it names. A box made before the
edit keeps its captured copy, so make a new box to get the change, or
run `lnk env setup` on the old one to run the image's stages there.

## Remove an Environment

No command deletes an environment, since it is your file. Removing it
means deleting what was made from it. [`lnk box delete`](../../../../box/docs/services/boxes/lifecycle/delete.md#what-deletion-does) deletes a box made
from it, [`lnk box image delete`](../../../../box/docs/services/boxes/image/README.md#delete-an-image) deletes its images, and
`lnk env approval remove` takes back each yes. Delete the file, and
nothing of Local Link's reads it again.

## Where Records Live

Local Link keeps environment records on the computer that creates
boxes, and copies the environment's files and service logs to the
machine running them. Every record is under `~/.lnk/safe/envs/`, and
sensitive records and specifications are written with mode 600, so only
your user reads them.

### On Your Computer

`~/.lnk/safe/envs/ids.toml` keeps each environment's ID by its file's
real path, and every ID minted here, which is never minted again.
`~/.lnk/safe/envs/boxes/<box>.json` records each cloud box made with an
environment, with its environment, its complete file and its cloud ID.
Reset uses it to find a box's environment, and a root service's reset
is allowed only on that box.

### On the Machine

`~/.lnk/safe/envs/envs/<name>/` holds the environment file and the
inputs and specifications it names, on a box or, for a run here, on this
computer. `~/.lnk/safe/envs/prepared/<name>` holds the hash whose image
stages finished there, so the
[preparation interface](more/protocol.md#captured-work-directories) skips
them while the hash still matches. `~/.lnk/safe/envs/runs/<label>.toml`
holds the sandbox specification a stage runs in, private to the runner.

### Service Logs and Names

`~/.lnk/safe/envs/logs/<name>-<service>.log` holds a started service's
output, so read it when a service fails or never becomes ready. A
service runs as `lnk-env-<name>-<service>.service` under systemd, Linux's
service manager, or as `link.env.<name>.<service>` under launchd on a
Mac.

## Command Environment

[`lnk help <command>`](../../../../../../docs/more/modules/guide.md#get-help)
lists the environment variables a command reads, as its
[`variables.toml`](../../../variables.toml) declares them. These are the
ones the environment module reads beyond its records.

### `LNK_JOB_OUT`

A run is given the folder whose files are kept after the run, when its
caller asks for one. Write results there when they must outlive the
machine, rather than assuming every file on it is kept.

### Isolated Test Inputs

`LNK_TEST_JOB_GROUP` selects the process group the run's profiler
samples. `LNK_TEST_WORKERS_SETTINGS` selects a trusted stand-in worker
settings file owned by the current user or root. Both are read only by
debug builds and ignored by release builds, so they can't replace
production worker policy.
