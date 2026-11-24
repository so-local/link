# Environment Contract

`link-env-contract` is the environment file, read strictly as every
specification is, its stages, and the wire requests and process clients other
modules reach an environment through. It also gives a run the secrets
its file asks for that you approved for it. Running it is `lnk-env`'s.

## The environment file

`file` reads it (`read`, from a path, every file it names checked;
`parse`, from text, by its type alone), refusing an unknown key, a missing specification or
input, and a value that can't be, each naming its line. It reads
version 1 (`VERSION`).

### What it names

It names no harness, no cloud and no box: its `name` (lowercase
letters, digits and dashes, which its boxes and images are named by),
its `os` (none, any, with an image for each), its `workdir` (where a
job's command and an agent start, from the home folder), `variables`
given to every run as they are, its stages and its services.

### A stage of the file

Each has `run`, a command line for `sh -c`; `sandbox`, the sandbox specification
it runs in, from the file's folder; `user`, who it runs as; `inputs`,
files whose contents count in the image's hash; `timeout`; and when it
runs, saved in the image or at every start.

### A service of the file

Each has `run`, a command line that runs until it's stopped; its
`sandbox` and `user` (`root` only on a box of its own); `reset`, a
command line putting it back as the image had it; and how it's ready:
a `port` on the machine's loopback taking a connection, or a `run` that
passes in its sandbox, waited on for its `timeout`, 60 seconds when the
file says nothing (`READY_TIMEOUT`).

### Its image's hash

`hash` is what an image is found by on a platform, an OS and a
processor. It covers the environment name, work directory, variables, and
saved stages, including their commands, users, sandbox files, and declared
input contents. Start-only stages and services do not change the image hash.

### How a job is kept apart

`isolation` reads it from what the job may do and who started it,
never chosen: a job needs a box of its own when someone other than you
started it, or when its command runs as root (`own_box_because`).

## Stages

`stage` is the one stage shape, shared by environments, jobs and
compositions: a stage alone or a parallel group of them
(`check_groups` checks the groups before any command starts, where
stages are sent from and where they run).

### `Run`

A stage as the machine it runs on is sent it: its name, `run`, its
sandbox specification as text so nothing is read there from a path (none for
`root`), a `protected` sandbox the runner verifies its specification against,
its `user`, and its own `timeout` within the whole run's.

### `Timeout`

`Timeout` bounds how long an operation may run, from a second to a
day. It accepts duration strings such as `30s`, `10m` or `2h`, which
callers apply within the whole run's deadline.

### Each step timed

A run's steps, its setup, a service made ready or a stage, are each
timed whatever their outcome (`Timed`), and `lnk env here` says each
outcome after the stage's seconds.

## `lnk env here`

`here` is what `lnk env here` is asked on its input's first line, once
it has said what to seal to on its output: the environment's name, its
file, the action, the secrets sealed to a key that run made, and the
mark each line saying how it went starts with, so a command's own
output can't pass for one.

### Its actions

`image`, the stages saved in an image; `setup`, the same with no image
saved, on a computer only those in a sandbox; `start`, the stages run
at every start then the services, which outlive it; `run`, those stages,
the services it needs, then the stages given, the first that fails
ending it with its exit code (124 at its timeout); `reset` of a
service; and `stop`, which stops services previously started in the background.

### A run's own folder

A `run` may be given variables, secrets by name, a folder made new for
its stages to leave files in (`Out`, from the home folder, its path in
a `LNK_...` variable), and a profile to sample.

### Where its files are

An environment's files are in Link's settings on a machine, which no
sandbox reads or writes (`files_dir`).

## Secrets a run is given

`values` gives a run the secrets its environment's file names that you
hold, each plain or sealed as its file names it, once you've said yes
(`approvals`). `of` takes an `Asker`, you at a terminal or no one, and
`of_yours` asks on this process's terminal where there is one; a name
with no yes that no one can answer is refused, naming the command that
approves it. A job's run (`of_job`, `of_unattended`) is given none.

### Approvals

`approvals` keeps your yes in the Safe (`~/.lnk/safe/envs/approvals.toml`),
each for a specification's real path, its `name`, the secret's ID (`secret:<id>`)
and the name the specification asks it by. It reads your secrets through the
secret contract (`list`, `for_you`), and leaves out an approval whose
secret is no longer kept, dropping it at the next write, so deleting a
secret removes its approvals.

### An image's build

An image's build is given nothing, since an image keeps what its stages
wrote, so a file naming a secret is refused there (`sealed`).

### Blanked in output

`Blank` replaces every value given in a text with `***` (`BLANK`), as
GitHub does in its logs: a backstop, not the wall, since a value
encoded on its way out isn't caught. Read as a stream, a value split
between pieces is still caught.

## An environment carried and run

The contract carries reviewed files and prepares them on the target
machine, while the remote runner and profiler report execution through
existing module interfaces.

### `Packed`

`Packed` carries the environment file, its declared text files and its image hash, never secret values. Capture holds authoring directories open and refuses symbolic links, hard links, non-regular files and paths outside that origin. `Prepared` is `lnk env prepare` and `import`'s answer, with paths on the machine that prepared it.

### On a box

`remote` runs the environment on a box from your computer, each step a
[`lnk box ...`](../../../../../../box/docs/services/boxes/README.md#boxes) command as a person would type it, and what runs there is
`lnk env here`, over SSH with [`lnk box run`](../../../../../../box/docs/services/boxes/ssh.md#run-lnk-on-a-box). A box it makes is never
signed in to its account, since an image build or a job runs code it
was handed.

### Preparing an Existing Machine

`remote::prepare_private` copies captured files to an existing machine and applies uncached preparation there, without selecting or publishing an image. Its `GuestLogin` says what the machine's login gives: `Root`, or `Ordinary` with one approved user. With `Ordinary`, `check_login` refuses any stage or service running as root or as another user before a file is copied. Startup and services follow preparation when requested. Existing root, secret and sandbox checks still apply. A borrowed machine can receive the current files without rerunning image preparation.

### A job's profile

`profile` samples a job's processes at each stage's edges and every
while: memory used and its peak, core-seconds, and bytes read and
written, each named as the measure contract names it, and none, never
zero, where it can't be read.

### Where it runs

`cli::at` resolves shared `Slots` into `At::Here`, `At::Box`, or
`At::Cloud`. `--here` selects this machine, `--on` names a box or a cloud
account that is one machine (its adapter says so), and `--cloud` selects a
cloud account with an optional zone. The caller supplies those slots while the contract carries the work.
