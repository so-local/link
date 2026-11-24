# Branches and Releases

Public contributions and reviewed source snapshots land on `main` by pull request.
Signed binary releases use the accepted source after maintainers activate the guarded release workflow.

## Source workflow

Source changes and binary releases follow the same reviewed history, with
separate checks for the source checkout and the programs signed from it.

### Public Source Workflow

The repository's `.github/workflows/release.yml` runs from the public root, calls the reusable public checks, then builds through the reusable `bundle.yml`.
Its guards permit signing only from `so-local/link` on `main`, with the repository variable `LNK_SIGNED_RELEASES` set to `true` after configuring release access.
Forks remain outside that flow.

### `main`

Where work lands and what's released. Branch from `main`, with one branch
and one PR per batch of related work, each change its own commit, and the
PR against `main`. Don't bump the version in these PRs. Say in the
description how to try the branch.

### A PR's Release notes

Fill in the PR template's **Release notes** (Breaking, Added or Fixed,
one line per user-visible change), and keep them and the description
current as commits are added, because the release PR reads them to pick
the version.

### Trying unreleased work

`git fetch && git checkout <branch> && cargo build`, or an install of
`lnk` and its modules from a branch with cargo, as
[Install `lnk`](../../../../docs/more/developing/setup/install.md) says.

## Cutting a release

A release is a pull request into `main` that bumps the version.

1. List what `main` adds: the PRs merged since the last release, and
   their Release notes.
2. Pick the version from the largest change, against the current
   `version` in the workspace's `Cargo.toml`. Before 1.0, **Breaking** or
   **Added** bumps the minor version (0.11.0 -> 0.12.0) and **Fixed**
   bumps the patch (0.11.0 -> 0.11.1). From 1.0, Breaking is major, Added
   minor and Fixed patch. If nothing is user-visible, merge without a
   bump, and nothing is published.
3. [Commit the bump](#the-bump) on a branch of its own. That
   commit is the only change the release makes itself.
4. Open a PR into `main` titled `Release v<version>`. Its body is the
   combined Release notes (Breaking, Added, Fixed), each line with its
   PR number.
5. Before merging, run [the checks](../../../../docs/more/developing/guide.md#before-you-push) on the
   branch's head. `checks.yml` runs them again after the merge and
   publishes nothing if they fail.
6. Merge. When signing is activated, the guarded workflow checks and builds `v<version>`, then waits for its protected release environment before publishing.

### The bump

`version` under `[workspace.package]`, then `cargo build` so
`Cargo.lock` follows. The release also ships Link Harness, Link's
tools, the OpenClaw, Hermes, and DeepSeek Harness
adapters and the connections' modules, which `lnk` installs only at its own version.
Every package inherits the workspace version.

### `set-version.sh`

From the workspace's root, `scripts/release/set-version.sh . <version>`
changes it and updates the one lockfile's workspace entries offline. Fetch
dependencies first if they are not cached. Each further workspace named
after the version takes the same version and updates its own lockfile, since
`lnk` embeds another workspace's component only at its own version.

### A hotfix

A **hotfix** is an ordinary PR into `main` with a patch bump and a Release
note under Fixed, so its merge publishes the fix.

## What a release publishes

A release publishes `lnk` with its modules, and signs it.

### `checks.yml`

Runs formatting, Clippy, tests, ShellCheck and binary builds on Linux for PRs into `main`, pushes to `main` and calls from `release.yml`.
Additional jobs check the sandbox on the release's supported Linux architectures and libc.
`website.yml` independently lints, tests and builds documentation/site changes on PRs.
`dependencies.yml` reads dependency evidence when requested, and
`models.yml` refreshes [the models file](#the-models-file) into a pull request.

### `lnk` (`release.yml`)

Publishes when the workspace version has no release yet. It makes
`v<version>`, the latest release of `so-local/link-releases`, with one
archive per program and platform (`lnk-core-<target>` and
`lnk-<module>-<target>`) for macOS and Linux, each on arm64 and x86_64,
with Linux arm64 built on GitHub's arm64 runner, all under one signed
`SHA256SUMS`. It's the [`link` bundle](../../../../modules/tooling/bundle/docs/tooling/bundle/README.md#link),
built by [`lnk bundle`](../../../../modules/tooling/bundle/docs/tooling/bundle/README.md#bundle) [through `bundle.yml`](#built-a-target-per-runner).

### Its agents

The same release carries Link's agents, `link-<agent>.tar.gz` for each
folder of [`agents/`](../../../../agents/README.md), which people download
and bring in with [`lnk archive import`](../../../../modules/tooling/archive/docs/tooling/archive/README.md#import). The `agents` job unpacks the
sealed bundle's own Linux `lnk` and its archive and agent modules, packs
each folder with [`lnk archive export --from`](../../../../modules/tooling/archive/docs/tooling/archive/README.md#export), and `publish` attaches the
archives. They're outside the signed `SHA256SUMS`: `lnk archive import`
checks an archive's every file before it writes one.

### Its modules

The modules are the workspace's: `lnk bundle` reads them from
`cargo metadata`, a module being a package's one `lnk-` program, and
leaves out [one a release holds back](../../../../packages/component/docs/more/libraries/types/component-metadata.md#what-a-release-ships),
which a staging or dev build keeps. A release ships nothing from
outside the workspace.

### What is signed on a Mac

`lnk bundle` signs a program for an Apple target with an ad-hoc
signature and the `entitlements.plist` beside its Cargo manifest, for a
program that runs only with an Apple entitlement. It signs no program
without one, and it refuses to build such a program where there is no
`codesign`.

### The models file

What Link knows of each model (its window, longest answer and prices)
is `modules/agents/model/contract/src/models/link.toml`, and a release
ships the file as committed. The by-hand `models.yml` refreshes it with
OpenRouter's current figures and opens a pull request into `dev` with
the change, which a maintainer reviews and merges like any other. A pull
request opened with a workflow's own token starts no other workflow, so
its checks run once a maintainer pushes to it or re-runs them.

### How users get it

Users get `lnk` from `curl -fsSL https://local.link/install.sh | sh`,
served by the website from `core/cli/install.sh` with that build's minimum
version. The bootstrap script stays independent of the download repository
whose signatures it checks. They get modules from [`lnk up`](../../../../docs/more/modules/install.md#pick-your-modules) and [`lnk mod add`](../../../../docs/more/modules/guide.md#add-or-remove-a-module), and
updates from [`lnk upgrade`](../../../../docs/more/modules/guide.md#upgrade).
The [update notice](../../../../docs/more/modules/decisions.md#how-does-lnk-say-a-newer-release-is-out)
uses the latest signed release information without delaying the command.

### Signed advisories

Maintain the complete advisory history in `scripts/release/advisories.toml`.
Sealing copies it as `ADVISORIES.toml` before calculating and signing
`SHA256SUMS`. The client accepts advisory bytes only after checking the
signed hash. The [release contract](../../../../core/framework/docs/more/modules/protocol.md#signed-release-advisories)
defines its fields and validation.

## How a release is signed

A release is a production build (`LNK_BUILD=prod`) compiled to trust
Link's signer, `link`, alone. `lnk bundle` seals it and signs its
`SHA256SUMS` with that signer, whose secret is the `bundle-link`
environment's `SIGNING_SECRET`. The signer was named `release`, and
its key is unchanged. [A secret kept here moves](../../../../modules/tooling/bundle/docs/tooling/bundle/signers/README.md#moving-links-signer-to-link) from
`~/.lnk/safe/bundles/signers/release/` to `signers/link/`.

### Built a target per runner

`bundle.yml` builds each target on a runner of its own, as `lnk bundle
create link --target <target>` does without the signer's secret: every
program, packed in its archive, its `lnk` compiled to trust Link's
signer, unsigned. Built for the runner's own machine, its
programs must run, say the bundle's version, and `lnk` say it's the
build it was made as.

### Sealing

One job then seals the parts with [`lnk bundle create --from`](../../../../modules/tooling/bundle/docs/tooling/bundle/README.md#create-a-bundle). It checks
that the parts are of one version and hold the same programs for every
target, adds `install.sh` trusting Link's signer alone, `VERSION`,
`COMMIT` (the source commit it was built from), `ADVISORIES.toml` and
`SHA256SUMS`, and signs them, checking the signature against the
signer before the job ends. `release.yml` then publishes the sealed
folder as it is.

### Only behind a reviewer

The sealing job reads the secret only in the `bundle-link`
environment, whose required reviewer approves each run and which only
`main` may use. Publishing runs only from `main`, in the `release`
environment, which holds the releases token. Repository checks verify
that every job using a secret declares those restrictions. GitHub must
also enforce each environment's reviewer and branch settings.

### Built without a cache

The jobs that build what's signed restore no cache, so a poisoned cache
can't alter a release, and the repository checks hold `release.yml` and
`bundle.yml` to naming none. A release's build compiles from the
checkout alone, slower than a cached one.

### Who trusts Link's signer

The installer, a release's updates and source-built programs trust the
public line in `scripts/release/link-signers`. The installer embeds the
same line, and the repository checks keep that copy aligned with its
source.

## A staging build

A staging build is a release in every way but its signer, so what
fetches a release (`install.sh`, `lnk upgrade`, `lnk mod add`) is
checked before the release. It's the [`staging` bundle](../../../../modules/tooling/bundle/docs/tooling/bundle/README.md#staging),
built from a commit by `lnk bundle` on your machine or a cloud session,
or by `bundle.yml` with `bundle=staging` and `signer=staging`, and
published to a bucket of your own laid out as a release.

### Its key

It's signed by the staging signer, whose public line is in
`scripts/release/staging-signers`. `lnk --version` says on its second line
when it's a staging build, and its `install.sh` trusts both signers. A
release's binary holds none of the staging signer's bytes.

### `LNK_BUILD`

A build's stage, `LNK_BUILD` says as it's compiled, read once
(`link_core::Build`), and `lnk bundle create --build` sets it: [`dev`](#a-dev-build), `staging` or `prod`, or unset for a
build from source; any other value, `release` among them, doesn't
compile. A release is the `prod` build compiled with Link's signer.

### What a build from source records

A build from source also records the
checkout it was built from (`link_core::CHECKOUT`, from
Cargo's `CARGO_MANIFEST_DIR`), where `lnk mod add` builds its
modules, and each program the commit it was built from, so programs
built apart are refused by name. A released build records neither.

### Which keys it trusts

Which signers `lnk` trusts follows from it
(`release::TRUSTED_SIGNERS`): a staging build trusts Link's signer and
the staging signer, so it installs staging builds and releases; a
production build, as every release, exactly the signer it was compiled
with (`LNK_PROD_SIGNER`), so a release never installs a staging build;
a build from source, Link's signer alone.

### What `release.rs` checks of it

That no workflow sets a build kind or names the staging signer's file,
that the staging signer isn't Link's, and that a staging build installs with its own
`install.sh` and is refused by a release's.

### Its version

Its own: the release's it comes before, then `-staging.` and its
commit's build ID, the commit's time in UTC to the minute, then `g` and
the start of its hash (`0.15.0-staging.202610131900.g1a2b3c4d5e6f`).
So the version names its commit, every machine that builds one commit gives it one version, a later
commit's is newer than an earlier one's, and the release is newer than
both, as `lnk upgrade` and a move's upgrade on a box order them.

## Install a staging build

`lnk bundle create staging` keeps the bundle in
`~/.lnk/data/bundles/staging/<version>/`, and a plain copy puts it in a
bucket, such as `aws s3 cp --recursive` of that folder to
`s3://my-builds/staging/<version>`.

### Install From a Private Bucket

On a machine without Link, copy the release's `download/v<version>`
folder from the bucket into `~/.cache/lnk-staging/<version>/download/`
first, then run its own `install.sh` from there, with `LNK_RELEASES_URL`
set to `file://` and that copy's folder, and `LNK_VERSION` to the
version. Where Link is installed, set `LNK_RELEASES_URL` to
`s3://my-builds/staging/<version>` and upgrade to the version.

```sh
lnk upgrade <version>
```

### On Google Cloud

The copies are `gcloud storage cp --recursive`, the second into
`~/.cache/lnk-staging/<version>/download/`. The copied `install.sh` checks
the signature on `SHA256SUMS` with the keys sealed into it, and each
archive against the signed sums, as from anywhere.

### From a public address

`curl -fsSL <url>/download/v<version>/install.sh |
LNK_RELEASES_URL=<url> LNK_VERSION=<version> sh`, checked the same way.

## A dev build

A dev build is a staging build whose second signer is yours, not the
staging signer: for testing your own commit on boxes without Link's
signers or a staging build.

### Why boxes need one

[`lnk box create`](../../../../modules/compute/box/docs/services/boxes/guide.md#create-a-box) installs `lnk` on the box from the releases this
machine's `lnk` reads, so a build from source, which has no published
release, can't install that source build on a box directly. A dev bundle
provides the signed programs from a local folder or a bucket of your
own. The three kinds: a merge into `main` builds releases, a maintainer's
staging build comes from any commit, and each contributor builds a dev
build from their own.

### Its key

An Ed25519 key of yours (`ssh-keygen -t ed25519`), never committed,
never Link's signer or the staging signer. `lnk --version` says on its second
line when it's a dev build.

### `LNK_DEV_SIGNER`

A dev build's `lnk` is built trusting Link's signer and the one
allowed-signers line in `LNK_DEV_SIGNER`, read as it's compiled
(`release::TRUSTED_SIGNERS`): a dev build without it, or with anything
but one `link-release namespaces="link-release" ssh-ed25519 <key>`
line, doesn't compile, rather than trust nothing or anything. No other
build reads it, so a release or a staging build never installs a dev build.

### What `release.rs` checks of it

That no workflow names `LNK_DEV_SIGNER`, that no
committed file holds a release signer that isn't Link's, the staging
signer or a test's, and that a dev build installs with its own
`install.sh` and is refused by a release's and by the `lnk` of a
release and of a staging build.

### Its version

A staging build's, with `-dev.`: the release's it comes before, then its
commit's build ID (`0.15.0-dev.202610131900.g1a2b3c4d5e6f`), ordered as a staging build's is.

## Make a dev build

`lnk bundle` makes one with a signer of your own, `dev`, which it
creates in the Safe the first time. For a dev box, the
[`devbox` bundle](../../../../modules/tooling/bundle/docs/tooling/bundle/README.md#devbox) builds what a box needs, and
[Developer Boxes](setup/dev-boxes.md) makes the box from it.

```sh
lnk bundle create --build dev --signer dev --target x86_64-unknown-linux-musl
```

### Choose a Destination

The bundle is kept in `~/.lnk/data/bundles/<name>/<version>/`, or the
folder `--out` names. [`lnk` reads it there](#where-releases-are-read-from) as `file://` or after a plain
copy to a private `s3://` or `gs://` location, read with your cloud
CLI's login.

### On a box

The box's `install.sh` is checked against your `lnk`'s keys before the
box runs it, and trusts the keys sealed into it, so it installs your
build. Your `lnk` reads the bucket with your `aws` or `gcloud` login
and copies the release to the box over its SSH, where it installs from
the copy; every `lnk` command run on the box follows the same
`LNK_RELEASES_URL`.

## Where releases are read from

`lnk` installs from Link's own releases unless `LNK_RELEASES_URL` names
others (`release::Source`). A release, a staging build and a dev build all
take all four: what's installed is trusted only as the signature on its
`SHA256SUMS` says, checked against the keys `lnk` was built with, never
by where it's stored.

### `https://<address>`

Read over HTTPS, at an address anyone may read: it holds no
login, `\`, query or fragment.

### `s3://<bucket>/<folder>` and `gs://<bucket>/<folder>`

Read with `aws s3 cp` or `gcloud storage cat`, as your `aws` or
`gcloud` is logged in.

### `file:///<folder>`

The folder here. Each part of a folder is letters, digits or `._~+_-`,
never `.` or `..`; anything else is refused.

### A bundle's own programs

A program run from a bundle's `bin/`, a folder `lnk bundle create` made
with `latest/download/VERSION` beside it, installs from that bundle
when `LNK_RELEASES_URL` names nothing (`release::Source::bundle_of`).
With the bundle's `bin/` first on `PATH`, `lnk box create`, `lnk agent
move` and `lnk mod add` install the bundle's build, as typed, and a
box made that way follows its copy of the bundle.

### A cloud's login

A cloud's CLI reads its own settings and files, and is given the
variables of its login, which Link hands no other program:
`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` and `AWS_SESSION_TOKEN`,
or `CLOUDSDK_AUTH_ACCESS_TOKEN` and `GOOGLE_OAUTH_ACCESS_TOKEN`.

### On a box

A box can't read a bucket with your login, or a folder here, so `lnk
box create` copies the release it installs to the box first: its signed
`SHA256SUMS` and signature, `VERSION`, `install.sh`, and every archive
for the box's processor, each checked against the signed sums, to
`~/.lnk/safe/releases/download/v<version>` there.

### Reading the copy

The box installs from that copy, checking each signature again, and
every `lnk` that [`lnk box run`](../../../../modules/compute/box/docs/services/boxes/ssh.md#run-lnk-on-a-box) runs there reads it as
`LNK_RELEASES_URL=file://$HOME/.lnk/safe/releases`. An `lnk` run on the
box itself follows the copy too, with nothing set: when
`LNK_RELEASES_URL` names nothing and `lnk` runs from no bundle, it
follows `~/.lnk/safe/releases` while that holds the signed sums of its
own version (`release::Followed::Copy`). It holds the version the box
was made with, so `lnk upgrade --check` there says there's no later
release in it to check for; a box for another version is made again.

### Whose copy

The copy followed is the one kept beside the `lnk` that runs, in the
installation folder whose `exec/bin` holds it, before the running user's
own. Root running the box user's `lnk` with `sudo`, as `lnk box user
add` needs, installs a missing module from the box user's copy, as it
finds modules beside that `lnk`.
