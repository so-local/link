# Environments Guide

Write an environment file, check it, then put it on a machine: a new box,
a box you have, or the computer you type on. This guide follows the file
from its first line to a box running its services, and
[Environment Life Cycle](lifecycle.md) explains what Local Link keeps of
it along the way.

```sh
lnk env check --spec env.toml
```

## Environment File

The file declares setup in order: values, stages and services. Save it
as `env.toml` in the repository it sets up. `os` left out means any OS,
and an image is built for each OS it's asked on. `workdir` is where a
service and a start stage begin, from the home folder. Variables aren't
secret: they're written in the file and given to every run. Stages run
in order, each in the sandbox its specification gives, those saved in
the image from the home folder and the rest from `workdir`.

```toml
# env.toml
schema = 1
name = "app"
version = 1
os = "ubuntu-24.04"
workdir = "app"

[variables]
REDIS_URL = "redis://localhost:6379/0"

[[stage]]
name = "system"
run = "apt-get install -y python3-pip redis-server"
user = "root"

[[stage]]
name = "clone"
run = "git clone https://github.com/me/app app"
sandbox = "clone.toml"

[[stage]]
name = "deps"
run = "python3 -m pip install --no-cache-dir --target app/.deps -r app/requirements.txt"
sandbox = "deps.toml"
inputs = ["requirements.txt"]
timeout = "10m"

[[stage]]
name = "pull"
run = "git -C app pull --ff-only"
sandbox = "clone.toml"
when = "start"

[[service]]
name = "redis"
run = "redis-server --dir /var/lib/redis --port 6379"
sandbox = "redis.toml"
ready = { port = 6379 }
reset = "sh app/scripts/reset-db.sh"
```

### What This File Sets Up

The `system` stage runs as root, outside any sandbox, only while the
image is built. The `clone` stage's sandbox writes `~/app` and its
`.git` and reaches github.com. The `deps` stage's `inputs` are files
whose contents count in the image's hash, and reaching its `timeout`
fails the build. The `pull` stage runs at every start and is never saved
in the image. The `redis` service is ready once its port takes a
connection, or `ready` can run a command such as `redis-cli ping`
instead, within 60 seconds by default.

### Rules Every File Follows

Every key is on the [Environment Specification](../../config/specs/env.md), and every command that reads a file holds it to these rules.

- **Paths** (`sandbox`, `inputs`) are from the file's folder, written
  relative, without `..` and through no link. A sandbox specification's own
  paths are best written from `~/`, as the box's home folder: the specifications
  are copied to the box, into Link's settings there.
- **A stage** names its sandbox specification, or `user = "alice"` to run as a
  [user of the box](../../../../box/docs/services/boxes/user/README.md#add-a-user) in their own
  sandbox, or `user = "root"` to run outside any sandbox as root, which only happens while the image is built, on
  a box made for that. A stage without `when = "start"` is saved in the
  image. Cloning is a stage: several repositories are several stages.
- **A service** runs in its own sandbox specification, or as root (`user =
  "root"`, Docker included) on a box of its own only. `ready` is a port
  of the machine's loopback that takes a connection, or a command, run
  in the service's sandbox, that passes. A service ready on a port
  serves it on the machine's loopback, so what runs reaches it with
  that port in its specification's `[localhost] ports`.
- **Names** of the file, its stages and services are lowercase
  letters, digits and dashes. A variable's name is capital letters,
  digits and `_`, and not one Link sets (`LNK_...`, `PATH`, `HOME`). A
  variable [named like a secret](#variables-and-secrets) (`API_TOKEN`) is refused.
- **An unknown key, a specification or input that's missing, or a value that
  can't be is refused** when the file is read, naming its line, by
  every command that reads it. A file of the shape Link read before
  (`[box]`, `[run]`, `[repository]`, a stage's `spec`, `root` or
  `every`) is refused naming what replaces each key, `[repository]`
  with its clone stage.

### Parallel Stages

A stage can instead contain a named `parallel` array of command stages.
Its members start together, each in its own sandbox, and all finish
before the next stage. A member cannot be another group. The group's
optional `timeout` bounds every member; each member may also have its
own timeout. Members use the same `when`. Their output lines start
with `[name]`. The first failure received is returned after the other
members finish. Grouping and member order affect the image hash.

## Set a Machine Up Without an Image

`lnk env setup` runs the stages an image would save, in order, each in
its sandbox, on a box you already have or on this computer, and saves no
image. On a box each stage runs as the file says, a `root` stage as
root. On your computer a `root` stage is refused before any stage runs,
since nothing there runs as root, as
[Security](security.md#a-box-or-your-computer) explains. So a file of
sandboxed stages alone sets up a checkout's dependencies on your
computer, each stage writing only what its sandbox specification lets
it. A file naming an OS is refused on a box of another, before anything
runs.

```sh
lnk env setup --spec env.toml --on web-1
lnk env setup here --spec env.toml
```

## Make a Box From an Environment

`lnk env create` makes a cloud box from the environment, building its
image when there's none for the file's hash, then starts its services
and waits until each is ready. Use a connected cloud account, and pass
`--box` with a box specification when you need a particular machine.
Left out, the box is the least machine that works on the file's OS.

```sh
lnk env create --cloud aws --spec env.toml --box box.toml --name web-1
```

### Billed Resources

Creation asks before making a billed box or image. Without a terminal,
`--yes` is required. Once you agree, the box and image steps inside it
receive that answer, so they don't ask again.

### Captured Inputs

[`lnk box create --env`](../../../../box/docs/services/boxes/guide.md#create-a-box)
makes a box from an environment too, with the box's own options. Both
capture the file and the files it names once, check the requested OS and
processor, reuse or build the matching image, and run the start stages
and services from that capture.

### Start a Box Again

The services outlive the command that started them and come back when
the box does. [`lnk box start`](../../../../box/docs/services/boxes/lifecycle/README.md#start-a-box) runs the `when = "start"` stages and the
services again, from the captured files.

## Reset a Service

`lnk env reset` puts a named service back as the image had it, with the
service's own `reset` command, for a fresh run without a new box. It
uses the environment file this computer recorded when it made the box.
For a box without that record, give the file with `--spec`. The reset
runs inside the service's sandbox on that box.

```sh
lnk env reset redis --on web-1
```

## Variables and Secrets

A variable in the environment file is given to its stages and services,
and may appear in an image or in output, so it's never private. Keep a
private value as a
[secret](../../../../../security/secret/docs/config/secrets/README.md#create-a-secret),
named under `[secrets]`. [Environment Approvals](approval/README.md)
record your permission before the environment receives a secret you
hold.

### Refused Secrets

A secret with no yes is refused by a run no one is there to answer,
naming it and the command that approves it, and an image's build is
given none. A variable named like a secret (a word `token`, `key`,
`secret`, `password` or `auth`) is refused when the file is read.

## More About Environments

Every flag is in `lnk env <command> --help`. [Environment Images](image/README.md)
explain building an image and the hash it's found by, and
[Environment Life Cycle](lifecycle.md#where-records-live) where every
record and log lives. [Troubleshooting](troubleshooting.md) explains file
refusals, missing approvals, build deadlines and services that never
become ready. [Security](security.md) has what Local Link guarantees and
its gaps, and [Decisions](decisions.md) why it works this way.
