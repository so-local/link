# Environments

> Environments Make Every Run Feel at Home

**An environment is one file that says what a machine has before work
starts, so every box made from it is set up the same way.** The file,
`env.toml`, names the OS, the values every run is given, the stages that
set the machine up, and the services it runs. Each stage, such as
cloning a repository, runs in its own sandbox.

<!-- quickstart -->
<!-- quickstart:steps -->

## Check an Environment File

Add the environment module. An environment file lives in the
repository it sets up, so run the rest from a folder of your own, such as
a new `link-env-example` folder.

```sh
lnk mod add env
```

Save this file there as `env.toml`. It is the smallest environment there
is, with a name, a version and nothing to set up yet.

<!-- quickstart-file: env.toml -->
```toml
schema = 1
name = "app"
version = 1
```

Check the file. The check reads it as every other command does, runs
nothing, and prints what it sets up and the hash its image is found by.
[Environment Files](../../../contract/docs/services/environments/files.md)
shows how the values, stages and services fit together.

```sh
lnk env check --spec env.toml
```

<!-- quickstart:end -->

## Create an Image

Connect your cloud account, then build an image from the environment.
An image is a box's disk with the environment's setup stages already
run, saved in your cloud account, so every box made from it starts
ready. Your provider bills you for the stored image. [Environment Images](image/README.md)
explain what an image holds, where it lands, and how Local Link finds it
again.

```sh
lnk cloud connect aws|gcp
lnk env image create --spec env.toml aws|gcp
```

## Create a Box

`lnk env create` makes a box from the environment in a connected cloud
account. It builds the image first when there's none for the file's
hash, then starts the environment's services and waits until each is
ready. Your provider bills you for the box and the image. The
[Environments Guide](guide.md#make-a-box-from-an-environment) explains what Local
Link captures and how a later start works.

```sh
lnk env create --spec env.toml --cloud aws|gcp
```

## More

An environment lives in a file you keep, and Local Link keeps records of
the boxes and images made from it. Its stages can be given secrets, and
other programs can call it.

### Follow an Environment's Life

[Environment Life Cycle](lifecycle.md) explains what an environment is
as a thing Local Link keeps: how it's created, checked, built into an
image and put on a machine, how it changes and how it's removed, where
its records live, and how it differs from a Docker container.

### Give Secrets to an Environment

You save your permission for a file to receive a secret with
[Environment Approvals](approval/README.md). The
[approval rules](approval/approvals.md) say what ties a permission to a file
and when a change needs a new yes.

### Find Limits and Fix Failures

[Security](security.md) describes how stages are isolated and how
secrets are handled. When setup or a service fails,
[Troubleshooting](troubleshooting.md) names the cause, and
[Decisions](decisions.md) gives the reasons environments use explicit
files.

### Call an Environment from Another Program

The [Interface](more/protocol.md) lists the commands other programs run. The
[Environment Contract](../../../contract/docs/services/environments/more/contract.md)
defines the file and the shared types those programs use.

<!-- docs-tree:start -->

## Docs

### Pages

- [Environments Guide](guide.md): Write an environment file, check it, then put it on a machine: a new box, a box you have, or the computer you type on
- [Environment Life Cycle](lifecycle.md): An environment is a file you keep, `env.toml`, and Local Link keeps a few records about it
- [Environment Approvals](approval/README.md): Manage the permission an environment file needs to receive secrets you hold
- [Environment Images](image/README.md): An environment image is a box's disk with the environment's setup stages already run, saved in your cloud account

### More

- [Interface](more/protocol.md): The environment module exposes process commands for running setup and services on another machine
- [Decisions](decisions.md): Why environments work the way they do
- [Security](security.md): An environment file determines setup, services and the values they receive
- [Troubleshooting](troubleshooting.md): An environment failure names the file line, stage or service involved

<!-- docs-tree:end -->
