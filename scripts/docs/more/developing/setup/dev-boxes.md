# Developer Boxes

Build your checkout and try it on a cloud box. The `devbox` bundle signs
your commit's `lnk` and modules with a signer of your own, and the box
installs them, leaving your installed `lnk` unchanged.

```sh
lnk bundle create devbox
```

## Before You Begin

Use macOS or Linux on ARM64 or x86-64, with the `bundle` and `box`
modules installed, Rust installed through `rustup`, Git, `tar`, `curl`
and OpenSSH 8.1 or later. Connect your cloud account in `lnk` first,
with permission to create and remove machines. A box costs money while
running, and its disk costs money until removed, and [`lnk box create`](../../../../../modules/compute/box/docs/services/boxes/guide.md#create-a-box)
asks before it makes one.

### Set Up a Mac

Install Xcode command-line tools and Rust through
[rustup](https://rustup.rs/) if they are missing. A box runs Linux, so
building its programs on a Mac cross-compiles them with Zig, through
`cargo zigbuild`. With [Homebrew](https://brew.sh/) installed, add both. Run `brew install zig`, then `cargo install cargo-zigbuild --locked`.

### Set Up Linux

Install Git, a C compiler, OpenSSH and `tar` through your distribution,
Rust through [rustup](https://rustup.rs/), and
[Zig](https://ziglang.org/download/) on your `PATH`. Add `cargo
zigbuild` for a box of another processor than this computer's. Run `cargo install cargo-zigbuild --locked`.

### Choose a Commit

Commit or stash changes first. [`lnk bundle`](../../../../../modules/tooling/bundle/docs/tooling/bundle/README.md#bundle) builds exactly `HEAD` from a
clean checkout, including on a feature branch. A commit is local and
need not be pushed, and files Git ignores don't make the checkout dirty.
The bundle's version names the commit, by its time and hash, so building
one commit twice gives one version.

## Create a Box

The `devbox` bundle builds `lnk`, the modules that make a box and reach
its cloud, and the `secret` and `sandbox` modules every box installs,
for this computer and for a Linux x86-64 box. The first build makes your
`dev` signer in the Safe. `lnk box create --bundle devbox` then makes
the box through the bundle's own `lnk`, which trusts that signer.

```sh
lnk bundle create devbox
lnk box create gcp --name gcp-1 --bundle devbox
```

### Choose the Machine

Ordinary creation flags apply, such as `--machine.gcp.size` for the machine type in Google Cloud's own table.

```sh
lnk box create gcp --name gcp-1 --machine.gcp.size e2-small --bundle devbox
```

### Choose ARM64

Name the targets yourself to build for an ARM64 box: `host`, this
computer, for the `lnk` that makes the box, and the box's own.

```sh
lnk bundle create devbox --target host --target aarch64-unknown-linux-musl
```

### Choose the Modules

`--program` names the programs a bundle holds in place of the
`devbox` list. Name `lnk`, `lnk-box`, your cloud's modules and the box's
`lnk-secret` and `lnk-sandbox`, then the modules you're testing.

```sh
lnk bundle create devbox --program lnk --program lnk-box --program lnk-cloud --program lnk-gcp --program lnk-secret --program lnk-sandbox --program lnk-agent
```

## Try a Later Commit

Commit the next change, build its bundle again and make a new box from
it. `lnk box create --bundle devbox` uses the newest `devbox` bundle. A
box keeps the version it was made with.

```sh
lnk bundle create devbox
lnk box create gcp --name gcp-2 --bundle devbox
```

## Connect and Clean Up

The bundle's `lnk` shares your normal Link account and box records, so
your installed `lnk` connects to the box by its name.

```sh
lnk box ssh gcp-1
```

### Remove the Box

Remove the machine and its disk when finished to stop their charges.
Nothing removes a development box on its own.

```sh
lnk box delete gcp-1
```

## Local State and Trust

Your `dev` signer is kept in `~/.lnk/safe/bundles/signers/dev/`, and
each bundle in `~/.lnk/data/bundles/devbox/<version>/`. No bucket,
public upload or replacement of your installed `lnk` is needed.

### Keep Your Signer

The signer's secret stays on your computer and is never copied to a box.
Keep it between builds, because each box trusts the signer its bundle
was made with. A released `lnk` refuses a dev build too. See [A dev
build](../releases.md#a-dev-build) for the signing contract and
[`lnk bundle`](../../../../../modules/tooling/bundle/docs/tooling/bundle/README.md#bundle)
for every way to build one.

### Recover From Failures

A bundle that fails part way is removed, so the next build starts clean.
A box whose setup fails stays until [`lnk box delete`](../../../../../modules/compute/box/docs/services/boxes/lifecycle/delete.md#what-deletion-does) removes it.
