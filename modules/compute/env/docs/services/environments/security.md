# Security

An environment file determines setup, services and the values they receive.
This page describes its trust boundaries, and
[Sandbox](../../../../../security/sandbox/docs/tooling/sandbox/security.md)
and [Boxes](../../../../box/docs/services/boxes/security.md) describe
the corresponding execution boundaries.

## What a file can do

Environment files describe commands that run on another machine. Strict
file validation and sandbox rules limit the files they can name and the
privileges those commands receive.

### Relative files

Unknown keys are refused. Named sandbox specifications and inputs must
stay inside the environment bundle. File paths resolve from the source
folder; field flags resolve from the invocation folder. Absolute paths
inside the bundle are rebased to child-relative names before validation.
`..` and symbolic links in a file or its parent folders are refused. [`lnk box copy`](../../../../box/docs/services/boxes/ssh.md#copy-files-to-a-box) follows no links either, except
system-owned links outside home that only root can create, such as
`/tmp` on a Mac.

### Sandbox specifications

Named sandbox specifications have the same checks as
[`lnk sandbox run --spec`](../../../../../security/sandbox/docs/tooling/sandbox/guide.md#sandbox-guide): they cannot write Link's settings, the home
folder itself or a dot folder in it. Copied specifications live in
Link's settings on the box, inaccessible to sandboxes, so one stage
cannot change a later stage's sandbox.

### Root stages and services

A stage or service with `user = "root"` runs through `sudo`, outside a
sandbox. Root stages run only during image builds on a dedicated box
removed afterward, never during startup. Root services require their
own new box and are refused on an existing box.

### Other box users

A run as another box user, created with [`lnk box user add`](../../../../box/docs/services/boxes/user/README.md#add-a-user), uses `sudo`
to enter that user's sandbox and home with the environment's variables.
A missing user or a request that also names a sandbox is refused before
execution. Computers refuse every run as another user.

### Root service resets

`lnk env reset` runs a root service's reset as root only on the dedicated
box made with that environment. The creating computer
records that box's cloud ID, tying the reset to a box already running
the environment's root service.

### Image credentials

Image builder boxes use [`lnk box create --lnk.login=false`](../../../../box/docs/services/boxes/guide.md#create-a-box). They never sign in
to your account, so the commands that build an image hold no account
token of yours.

## A box or your computer

The machine boundary permits root setup only on boxes. Export validation
also limits which environment a computer accepts, independently of
whether its image hash matches.

### Exported files

The [environment transfer interface](more/protocol.md#captured-work-directories)
carries the environment file and named inputs, limited
to 1024 text files and 8 MiB. Import stages them privately, refuses
absolute paths and `..`, checks the environment and hash, and rejects
extra files before installing any. A computer requires an exact match
to its reviewed copy, including start stages, services and every input;
the image hash alone is insufficient.

### Machine marker

Only a box builds images or runs root stages. `lnk` reads the machine's
kind from `/etc/lnk/kind.toml`, represented by
`link_machine::machine::Kind`; a file cannot override that boundary.
The box's first boot writes it, with the OS of the box's specification,
which its image is built for.

```toml
# /etc/lnk/kind.toml
kind = "box"
os = "ubuntu-24.04"
arch = "x86_64"
```

### First boot

`lnk box create` supplies the marker through cloud-init's `write_files`
at first boot, before user commands. Root owns the file and its folder,
with file mode 644. Boxes made from images receive the marker again.
An adapter without cloud-init creates no marker, so `lnk` treats that
machine as a computer.

### Missing or untrusted markers

No marker, or `kind = "computer"`, means a computer; nothing is written
on a personal machine. The marker and every parent folder must be root
owned and unwritable by others, except sticky folders such as `/tmp`.
An untrusted marker is refused rather than interpreted. A program or
user cannot mark a computer as a box without already having root on
that machine.

### User namespaces

A user namespace such as `unshare -Urm` can make a mounted marker appear
root owned. `lnk` refuses it unless `/proc/self/uid_map` maps every user
ID to itself. This is best effort: the namespace can also replace
`/proc`. Even then, root runs use `sudo`, which gains no machine root
in that namespace, so a forged box runs only non-root stages. A
sandbox's `/etc` contains only what its programs need.

### Root execution checks

On a box, `lnk env image create here` permits root image stages and
the target runner permits root runs, including services and jobs on their
own boxes. On a computer, that runner refuses image builds and any
request with root work before execution, and checks again before each
root run. Root work uses `sudo -n` with privileges the box user already
has; the computer's sudo is never requested.

### Test substitutes

Only debug builds read `LNK_SUDO`, a substitute for `sudo`, and
`LNK_TEST_MACHINE_KIND`, a user-owned replacement marker used to test a
stand-in box. Release builds ignore both substitutions.

## Where values go

Variables are ordinary configuration; secrets require grants bound to a
specific file. The two connections have different storage and execution
rules so an image cannot retain a granted secret.

### Variables

Variables appear as text in the environment file, images, output and
records. A name containing a secret-like word (`token`, `key`, `secret`,
`password` or `auth`) is refused, helping keep credentials out of this
ordinary configuration connection.

### Your yes

An environment you run acts as you: it's given a secret its file names
only once [you've said yes](approval/approvals.md), kept in your Safe by the env module for the
file's real path, its `name` and the secret's ID, never in the file. A copy, a moved file, a changed `name` or a
secret made again asks again. A run no one is there to answer is refused
a secret with no yes, and a job, which runs with no one there, is given
none.

### Secret recipients

Stages receive the secrets you approved. A sealed value reaches them
only as a placeholder, which the proxy fills for its host. Services and
image builds receive no secrets, so a built image cannot keep one.

## Gaps

Environment validation does not make arbitrary commands trustworthy.
These remaining boundaries matter when reviewing an environment file
or allowing setup to change files on a machine.

### External root services

Root services execute the environment file's commands on its dedicated
box. A file you did not write requires `--others` and a box of its own;
review those commands before permitting it to run.

### Existing symbolic links

Execution follows `workdir` and stage paths through links already on
the machine. If a stage changes `workdir` into a link to another folder,
subsequent work begins in that destination.
