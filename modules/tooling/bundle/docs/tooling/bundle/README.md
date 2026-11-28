# Bundle

> Build Link's Programs Into One Signed Folder

`lnk bundle` builds any set of Local Link's programs from source into one
signed folder that every installer already reads. Every bundle has one
stage, development, staging or production, and the stage says what the
bundle's `lnk` trusts. Use `lnk bundle` to make Local Link's release, a
staging build, a dev build for your own boxes, a production bundle of your
own, or a bundle of your own programs beside Local Link's. The
[Bundle Specification](../../config/specs/bundle.md) lists the fields, as a file or
as flags.

```sh
lnk bundle list
```

## Before You Begin

Run `lnk bundle` from a checkout of Local Link's source with every tracked
change committed or stashed. A file Git doesn't track, such as a log, is never
built, so it doesn't stop a build. You also need
Rust installed through `rustup`, Git, `tar`, `curl` and OpenSSH 8.1 or
later, and `zstd` for an outside program from a Debian package. Building for another machine's target also needs Zig and `cargo
zigbuild`, as the [dev boxes guide](../../../../../../scripts/docs/more/developing/setup/dev-boxes.md#before-you-begin)
sets up.

## Prebuilt Bundles

The module comes with the bundles Local Link's own work uses. Each is a specification
file you create by name, and `lnk bundle list` lists them.

### devbox

The `devbox` bundle holds this computer's `lnk`, the modules that make a
box and reach its cloud, and the `secret` and `sandbox` modules every box
installs beside `lnk`. It also holds what an agent moved there runs: `agent`,
its `harness`, `model`, which serves its model calls, and `mcp` with the
`manual` it needs. It builds for this computer and for the box, and your own
`dev` signer signs it. The programs for this computer land in the bundle's
`bin` folder, ready to [make a dev box](#make-a-dev-box).

### link

The `link` bundle is Local Link's public release. It holds `lnk` and every
module the release ships, for every target. It is a production build
compiled to trust Local Link's signer, `link`, and signed by that signer.
CI builds it one target per runner and [seals the parts](#build-in-ci), and
the release workflow publishes it.

### prodbox

The `prodbox` bundle makes a production box of your own. It holds `lnk` and
every module Local Link's release ships, for this computer and Linux boxes,
and your own `prod` signer signs it. Its `lnk` trusts that signer alone, so
the box installs only what you signed and never Local Link's own releases.

```sh
lnk bundle create prodbox
lnk box create aws|gcp --name prod-1 --bundle prodbox
```

### staging

The `staging` bundle holds `lnk` and every module, for Linux on x86_64 and
arm64, where boxes run them. The staging signer signs it, so you can try a
commit on boxes before it ships. Copy it to a bucket, and it installs as a
release does.

## Signers

A bundle is signed by a signer you keep by name in the Safe, and the
bundle's stage says which signers its `lnk` trusts. [Signers](signers/README.md)
shows how to make one, where each is kept, and which signers are Local
Link's own.

## Create a Bundle

`lnk bundle create` builds the programs for each target. It builds each
workspace's programs in one Cargo build, so the crates they share compile
once. Then it packs each program in its own archive, and seals and signs
the archives. Name the workspaces to find programs in, the programs, and
the targets. Leave them out for this checkout, `lnk` and every module, and
this machine.

```sh
lnk bundle create --target x86_64-unknown-linux-musl --signer dev
```

### Choose the Programs

`--program` takes a program's file name, such as `lnk-measure`, and
repeats for more. `--source` adds another workspace to find programs
in. A program two workspaces build is refused, since a bundle holds one
of each.

### List Modules From Other Workspaces

The bundle's `lnk` comes with a list of the modules it can install and
run, read from its own workspace as it's compiled. `--components` names
a module's folder in another workspace, and repeats for more, so `lnk`
lists that module too and answers its commands on the machine the
bundle installs to. Build the module's program as well, with
`--source` and `--program`. A `prod` build lists only a module that is
released. `lnk` lists nothing else, whatever the shell or folder the
bundle is made from sets.

### Choose the Targets

`--target` takes one of Local Link's release targets, each an operating
system on a processor architecture, and repeats for more:
`x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` for Linux on
x86_64 and arm64, and `x86_64-apple-darwin` and `aarch64-apple-darwin` for
macOS on each. `host` stands for this computer's own. Any other target is
refused, from a specification file or a flag alike, since no installer would find
its archives. Each program is built for each target, so every target you
add makes the build take about as long again.

### Choose Its Stage

`--build` names the bundle's stage, `dev`, `staging` or `prod`. The stage
says which signers the bundled `lnk` trusts and how its version reads. A
staging or dev build is built in a worktree of your commit with its version
set, so your checkout is left as it is. The version carries the commit's
build ID. The build ID is the commit's time in UTC to the minute, which
orders builds, then `g` and the first twelve characters of the commit's
hash, which names the commit. So every machine that builds one commit gives
it one version.

#### Find the Build Worktree

The worktree is kept, one per checkout, at `bundle/source` in the first
source's Cargo target folder, such as `link-public/target/bundle/source`,
and the build's output stays in that target folder too. Each build
moves the worktree to its commit with `git checkout --detach --force`
and removes every file the commit doesn't track, so it still builds
the commit and never your uncommitted changes.

#### What a Build Reuses

A file the two commits share keeps its time, so Cargo reuses what it
built from that file when nothing else changed. Every member's version
still changes with each commit, so a build of a new commit compiles the
workspaces' own crates again, and reuses only the third-party crates. A
second build using the same worktree is refused while the first holds
`bundle/source.lock`.

#### Build the `dev` Stage

The bundled `lnk` trusts the bundle's own signer and Local Link's, at a
version such as `0.0.1-dev.202611090720.g1a2b3c4d5e6f`. A signer of
your own signs it, never Link's nor the staging signer, so no released
or staging `lnk` installs it.

#### Build the `staging` Stage

The bundled `lnk` trusts Local Link's signer and the staging signer, at
a version such as `0.0.1-staging.202611090720.g1a2b3c4d5e6f`. Only the staging
signer signs it. That signer is the one the source's
`scripts/release/staging-signers` names, which the bundled `lnk` is
compiled to trust. A source of your own may
name a signer of yours there, kept under any name, and its staging
builds then install each other's, and Link's, without Link's signers'
secrets.

#### Build the `prod` Stage

The bundled `lnk` trusts exactly the signer it's compiled with, the
bundle's own, and the bundle has the workspace's version, such as
`0.0.1`. Local Link's release is the production build signed by Local
Link's signer. A production bundle of your own, signed by a signer of
yours, makes boxes that install nothing else. The staging signer never
signs a production bundle.

### Sign the Bundle

`--signer` names who signs the bundle: `link`, `staging`, or a signer of
your own by name or by an allowed-signers file. `--signing-secret` names the
signer's secret file. It is a runtime flag and never a field of a specification.
The secret is read only once every program is built, and the signature is
checked against the signer before the command ends. A wrong secret fails
here, not on the machines that install the bundle.

#### Unsigned Builds

Dev and production builds require a signer because their programs embed its
trust, including unsigned parts built without Link's signing secret. A staging
part may omit a signer or use `--signer none` for later sealing with
`--from`.

### Add Outside Programs

`--outside` names a TOML file declaring programs Local Link doesn't build,
each with its download for every target and the SHA-256 sum it must
have. `take` names the file to take from an archive.

```toml
# outside.toml
[[program]]
name = "caddy"
version = "2.10.2"

[program.targets.x86_64-unknown-linux-musl]
url = "https://github.com/caddyserver/caddy/releases/download/v2.10.2/caddy_2.10.2_linux_amd64.tar.gz"
sha256 = "5c218bc34c9197369263da7e9317a83acdbd80ef45d94dca5eff76e727c67cdd"
take = "caddy"
```

### Write a Specification

Every field flag is a field of one specification file, so a bundle you make
often is one file and one command, `--spec FILE`. A field flag replaces the
field it names, and a file's relative paths start from the file's folder. `lnk bundle spec show` prints the specification a file, a built-in
bundle or flags make, without building anything.

```toml
# bundle.toml
schema = 1
source = ["."]
program = ["lnk", "lnk-box"]
target = ["aarch64-apple-darwin", "x86_64-unknown-linux-musl"]
build = "dev"
signer = "dev"
```

```sh
lnk bundle create --spec bundle.toml
lnk bundle spec show devbox --target host
```

### Seal Parts Built Elsewhere

A build for several targets can run on several machines, and each machine
makes an unsigned part. One machine that holds the signer's secret seals
the parts with `--from`.

```sh
lnk bundle create --target aarch64-apple-darwin --build prod --signer link --out part-mac
lnk bundle create --from part-mac --from part-linux --signer link --signing-secret link-secret --out dist
```

## Make a Dev Box

A dev box runs your checkout's commit. The `devbox` bundle builds the commit
for this computer and the box. [`lnk box create --bundle devbox`](../../../../../compute/box/docs/services/boxes/guide.md#create-a-box)
then makes the box from the newest `devbox` bundle, through the bundle's own
`lnk`, which trusts your `dev` signer.

```sh
lnk bundle create devbox
lnk box create aws|gcp --name gcp-1 --bundle devbox
```

Connect your cloud account first, as [Platforms](../../../../../cloud/cloud/docs/services/cloud/README.md)
describes. To try a later commit, build its bundle again and make a new box
from the bundle the same way.

## Build in CI

The repository's `bundle` workflow builds any bundle from the branch you
start it on. Each target builds unsigned on its own runner, then one job
seals the parts with `--from`. Only that job receives the signer's secret,
through the `bundle-<signer>` GitHub environment. Set the environment's
required reviewers and branch restrictions in GitHub's environment settings.
`gh workflow run bundle.yml --ref my-branch -f bundle=staging -f
signer=staging` starts a build, and `gh run download --name bundle --dir
bundle` brings its bundle here.

### Signer Environments

Each signer's environment supplies `SIGNING_SECRET`. Local Link's is `bundle-link`.
The workflow selects that environment, while GitHub's configured protections
determine which branches may use it and whether review is required. The release workflow calls the same workflow for
the `link` bundle, then publishes what it sealed. `file` names one more file from the
repository to seal with the bundle, as `--file` does.

## Check a Bundle

`lnk bundle show` lists each program with its target and the target's
architecture, `x86_64` or `arm64`, so you see which machines the bundle
installs on. `--json` gives the same rows to another program. When
`SHA256SUMS` is present, `show` checks the listed files and rejects extra
files. It checks an existing signature against `--signer`, or classifies it
against Local Link's signer when you omit `--signer`.

```sh
lnk bundle show devbox --signer dev
```

### Unsigned Parts

You can inspect a part without sums or a signature when you omit
`--signer`, and the result says those checks were not supplied. An explicit
`--signer` refuses a bundle with no signature. A successful unsigned
inspection does not show that the part is a signed release. Only a sealed
bundle carries the signed checksums its installer requires, so keep the
reported signature state with the inspection result.

## Install a Bundle

A bundle is a folder in the published release layout,
`download/v<version>/` and `latest/download/`, holding each program's
archive, `install.sh`, `VERSION`, `COMMIT`, `SHA256SUMS` and its
signature. [Every installer reads it](../../../../../../scripts/docs/more/developing/releases.md#where-releases-are-read-from) as it is, from a folder here or
after a plain copy to a bucket, through `LNK_RELEASES_URL`.
An installed `lnk` installs only what a signer it trusts signed. The
bundle's own programs, in its `bin/`, install from the bundle itself, so
a box they make gets the bundle's build.

### Where Bundles Are Kept

A bundle goes to `~/.lnk/data/bundles/<name>/<version>/` unless `--out`
names another folder. The name is the built-in bundle's or the specification file's.
`lnk bundle show devbox` finds the newest of a name, and a version finds
that one.

<!-- docs-tree:start -->

## Docs

- [Signers](signers/README.md): A signer is what signs a bundle

<!-- docs-tree:end -->
