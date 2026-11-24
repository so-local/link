# Environment Files

An environment file says what every machine made from it has before work starts. This page explains how its parts fit together; each field is in the [Env Specification](../../../../docs/config/specs/env.md) reference.

## Order and Reading

The file's order is its name and version, `os` and `workdir`, then the values every run is given, then the stages that set a machine up, then the services it runs.

### Validation

Every command that reads an environment takes its file as `--spec
<file>`, with any field as a flag that is its key path laid over it, such
as `--variables.REGION eu-west-2`; a stage or service is given whole, as
an inline table. A file is at most 64 KB; unknown keys, empty tables and
invalid values are refused with the source line named.

## Secrets

The file names secrets; their values and your yes to giving each are this machine's.

### Give a secret

Run [`lnk secret create NPM_TOKEN`](../../../../../../security/secret/docs/config/secrets/README.md#create-a-secret), then `lnk env approval add NPM_TOKEN
--spec env.toml` to say yes to giving it to that exact file, named
`app`. Your yes is for the file's path, its `name` and the secret's ID,
so a copy or a move asks again:
[Approvals](../../../../docs/services/environments/approval/approvals.md).

### When grants are read

A missing grant is refused before running the file, naming the secret.
[`lnk env check`](../../../../docs/services/environments/README.md#check-an-environment-file) reads files naming secrets without requiring grants.
Stages and jobs receive granted values; services receive variables
alone. Image builds receive no secrets because images keep what their
stages write.

## Machines, users and runs

An environment defines setup shared by agents, jobs and interactive
work. Machines, users and runs have separate owners, so the same setup
can be carried without changing who owns the machine or starts work.

### Machine

A machine is your computer, yours alone and never an image, or a box
made from an image or none and saved as one. `here` names whichever
machine runs `lnk`. The box module writes `/etc/lnk/kind.toml` as root
on a box; without it the machine is a computer.

### User

A user belongs to the machine: you on your computer, a box's own user
such as `ubuntu`, `root`, or another user of the box. Each run names who
it runs as within that machine's permissions.

### Environment

An environment is the values, stages and services every box made from
it receives, and the image those stages build. This file and the env
module own that setup; the box module owns the machine itself.

### Run

A run is a command, as a user, in a sandbox, on a machine. Its caller
owns when it starts and ends. A stage and a job's stage share one type;
an agent is a run that stays up at `workdir`, with its own sandbox
file, the environment's values and its services.

### Kinds of run

Runs differ in when they start, what ends them and which user they
may use. Each kind retains the same machine, user and sandbox boundary.

- **Stage:** starts while an image is built or at each start, and ends by itself. It runs as the machine's user, or as `root` during an image build.
- **Service:** starts at each start and runs until stopped; `ready` reports readiness. It uses the machine's user, or `root` on a box of its own.
- **Job stage:** starts with its job and ends by itself or at a stage or job timeout. It uses the machine's user, or `root` on a box of its own.
- **Agent:** starts when asked and runs until stopped, as the machine's user, within its own sandbox.
- **SSH session:** starts when a person connects and ends when they leave, as a user of the box in that user's sandbox.

### Where setup runs

Only a box takes an environment whole: its image is built on a box
made for it. Your computer runs start stages, services and jobs, but
never build stages or an image build. Current limitations are listed
under [Current Limits](#current-limits).

## Parallel stages

A stage is one command, or a named group of commands:

```toml
schema = 1
name = "example"
version = 1

[[stage]]
name = "checks"
timeout = "10m"
[[stage.parallel]]
name = "lint"
run = "./lint.sh"
sandbox = "lint.toml"
[[stage.parallel]]
name = "test"
run = "./test.sh"
sandbox = "test.toml"
```

Members start together, each in its own sandbox. Output lines start
with the member's name. A failing member lets its peers finish, then
ends the build or job with the first failing member's code; subsequent
stages do not run. A group timeout applies to every still-running
member, within the whole run's timeout. A member cannot be a group,
and a stage cannot name both `run` and `parallel`. Groups are nonempty;
the group and member names are unique across the file. An environment
group's members have the same `when`; its image hash includes the
grouping, member order and group timeout.

## Image

An image is a box's alone, saved by [`lnk env image create`](../../../../docs/services/environments/image/README.md#create-an-image). Its hash is made
from what builds it, and nothing else:

- the file's `name` and `workdir`, its `os` (or, left out, the OS of
  the box it's built on), and that box's processor;
- the variables;
- each stage saved in the image: its `run`, its `user`, its sandbox
  specification's contents and its `inputs`' contents.

A changed lockfile makes a new image; a comment, a service or
a stage run at every start doesn't, and a box made from the image is
given the file as it is now.

### Build cleanup

The box an image is built on is removed once the image is saved or the
build fails. Should `lnk` never get to it, its cloud ends and deletes
it at its deadline: the build's `--timeout` and a quarter hour, or four
hours without one. The timer that ends a box on AWS is left out of the
image, so a box made from it never ends at that deadline.

## Commands that read it

The same file is read strictly by validation, image, setup and startup
commands.

### Check and print

`lnk env check --spec env.toml` reads the file and its named files and
prints the image hash. [`lnk env spec show --spec env.toml`](../../../../docs/config/specs/env.md#use-it) prints the file
with defaults. Serialization reads the file or this computer's record,
never a box's writable copy; paths remain relative to the folder whose
specifications and inputs travel together.

### Build and start

`lnk env image create aws --spec env.toml` builds on a dedicated box.
`lnk env image create here --spec env.toml` builds only on a box, for the
computer that made it to save. [`lnk box create aws --env env.toml --spec box.toml`](../../../../../box/docs/services/boxes/guide.md#create-a-box)
makes a box from the image, building first when necessary.

### Set up and reset

[`lnk env setup --spec env.toml --on app-1`](../../../../docs/services/environments/guide.md#set-a-machine-up-without-an-image) runs image stages without
saving an image, refusing root on a computer before anything runs.
[`lnk env reset redis --on app-1`](../../../../docs/services/environments/guide.md#reset-a-service) restores a service, and
service port forwarding opens its port as a
tunnel.

### An environment carried to another machine

The environment preparation interface captures declared text inputs and
installs them on the target before work starts. [Captured Work Directories](../../../../docs/services/environments/more/protocol.md#captured-work-directories)
defines export, validation, preparation, and the target-local paths returned
to a caller.

## What isn't in it

Other contracts own the machine, work to run and access policy. The
environment file links to those contracts instead of redefining their
settings.

- **The machine's size and cloud:** a [Box Specification](../../../../../box/docs/config/specs/box.md)'s, named by
  `lnk box create --env` or [`lnk env create --box`](../../../../docs/services/environments/README.md#create-a-box).
- **Who comes to a box:** never the environment's, which says what a
  machine has, not who comes.
- **An agent's sandbox:** its own file
  (`~/Link/Safe/Agents/<agent>/sandbox.toml`, `lnk agent start
  --sandbox`), never made from a stage's.
- **Secrets:** granted to an environment's file, and given to its
  stages and jobs, never its services or images.

### Current Limits

The file is read whole. A box made to build an image is the least
machine that works on the file's OS, so `lnk env image create` builds
for that OS and `x86_64`. A box made from the environment takes its size
and processor from `lnk box create --env` or `lnk env create --box`.

### Design rationale

The environment separates setup from machine allocation and work to run.
Why it works this way: [Decisions](../../../../docs/services/environments/decisions.md#the-environment-specification).
