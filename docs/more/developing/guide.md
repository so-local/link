# Developing Guide

Build `lnk` from source, test it, and ship a change to `main`. Where new
code goes is in [Architecture](../architecture/README.md).

## What you need

You need Rust and `curl`. Rustup installs the version the repository
pins, from its `rust-toolchain.toml`, at your first `cargo`. The checks
before a push also need `shellcheck`, and on Linux the sandbox's tests
need `bubblewrap` and `openssh-server`.

## Build and use it

`cargo build` selects the core by default and puts `lnk` in
`target/debug`, where `target/debug/lnk --help`, `target/debug/lnk
module list` and `target/debug/lnk --link-registry` show what it built.
Other Rust packages build when selected. To run an agent with the parts
it needs from this checkout, follow
[Run a Development Agent](README.md#run-a-development-agent).
[Install `lnk`](setup/install.md) puts `lnk` on your PATH, from source or
a release.

## Test

Tests run the workspace's programs from `target/debug`, so build them
first. Local services and fakes need no paid account. `cargo test` tests
the core. For every package, run `cargo build --workspace --bins`, then
`cargo test --workspace`. `cargo test -p link-docs-check -p
link-docs-check-code` runs the docs checks alone.

### Fast Checks

Fast checks are the goal: every change waits on them. Measure expensive
tests and preserve the behavior they prove when reducing their cost.
A library builds doc tests only when its docs
hold a Rust example: one with none sets `doctest = false` under `[lib]`,
and the first example added takes it out. The docs checks' `doc-tests`
rule holds both.

### Which packages build

The workspace's default members are the core. Select another
package with `-p <crate>`, or every member with `--workspace`.
Cargo resolves features for the selected packages. Expanding a selection
can compile a shared dependency again with additional features. A core
build never compiles a connection, harness adapter or the tools' document
extractors.

### Measure test execution

Use [Test Timing](../../../tests/timing/docs/more/developing/testing/test-timing.md)
to turn saved Cargo output into a module breakdown and a standalone
chart. Keep the test command's status and machine settings with the
report. Compilation and test execution are separate costs, and a
failed or partial run is not a complete performance baseline.

### Count the programs a command starts

Every program `lnk` or a module starts costs the user that program's
start, so a command starts only what it needs. A test holds common
commands to a budget, the most programs each may start in a fresh home:
[`spawns.toml`](../../../core/cli/src/spawns.toml). It runs each under
`strace` (a Linux tracer of system calls), counts every `execve`, and
names each program started when a budget is exceeded. A machine without
a working `strace` skips it through the testing kit's `program` gate. Run `cargo build --workspace --bins`, then `cargo test -p link-cli spawns`.

#### Count Programs by Hand

Count one command by hand the same way, then read `trace.txt` for which
programs started which. `strace -f -qq -e trace=execve -e signal=none -o
trace.txt -- lnk agent list` records each program started, and `grep -c
'= 0$' trace.txt` counts those that ran.
A build from source also starts each module once with `--link-describe`
[to check its commit](../../../core/framework/docs/more/modules/protocol.md#built-from-the-same-sources).
A release build starts none of those, so it starts fewer than a budget
allows.

### A test's home, services and Keychain

A test that runs `lnk` gives it a temporary `HOME` and
`LNK_SERVICES=off`. On a Mac, Link always keeps secrets in the Keychain,
so the testing kit's child environment also points `LNK_SECURITY` at a
stand-in for Apple's `security` tool, [`link_test_kit::keychain`](../../../packages/test-kit/docs/more/developing/testing/testing-kit.md#the-keychain-and-users).
A service's or a Keychain item's name doesn't depend on `HOME`, so
without them a test on a Mac stops or replaces your agent and writes
your Keychain.

### The sandbox on Linux

The sandbox's tests need bubblewrap, as [Gotchas](#gotchas) says. On
Linux with bubblewrap, the agent tests run the mini harness in the
sandbox, with its endpoint served out by its proxy (`bridge.rs`).

## Debug information

Every workspace owns one profile;
`CARGO_PROFILE_DEV_DEBUG=true` enables debug info for a build, or cargo's
`--config 'profile.dev.package."<name>".debug=true'` for one package.

## Write a test

A unit test sits beside its code, and a test that runs `lnk` or a
module is a module of its package's test program, which declares itself
`link_test_kit::program!(system)` so each test runs in a process of its
own. Build the workspace's programs before their tests, since the tests
run them. Every test uses the [testing kit](../../../packages/test-kit/docs/more/developing/testing/testing-kit.md)
for its folders, programs, waits, ports and skips, and
[Testing](testing/README.md) tells how testing works as a whole.

### Names and the Network

A test whose sandbox's proxy reaches a host by name gives the name an
address with `LNK_TEST_RESOLVE` (`example.com=192.0.2.1`), so it passes
on a machine without DNS, behind an HTTPS proxy alone (Claude Code on the
web). One whose proxy speaks TLS to a host of its own gives the proxy
that host's authority with `LNK_TEST_CA` (a PEM file), and reaches it
through an upstream given with [`lnk sandbox run --upstream http://127.0.0.1:<port>`](../../../modules/security/sandbox/docs/security/sandbox/guide.md#sandbox-guide).

## Gotchas

Keep the checkout outside `/tmp`, which sandbox specifications cannot read.
Use [Development Environment](setup/agents.md) for shared setup, build limits, credentials
and the sandbox's platform requirements. A missing sandbox capability is
reported explicitly; it does not establish sandbox coverage.

## Before you push

Run `cargo session-check -p <package>`, naming each package the change
touched: the docs and repository checks, formatting, Clippy, the
workspace's programs and those packages' tests, as
[Testing](testing/README.md#check-a-change) says.

### The Full Checks

`cargo full-check` runs every check over the whole workspace, and it
passes before an initiative merges. `checks.yml` runs the checks again
on a push to `main`, before a release is published. The steps are the
workspace's `.cargo/checks.toml`.

### Sandbox Tests on arm64

`checks.yml` also runs the sandbox's unit tests on arm64 Linux, which
has a release of its own. After a change to the sandbox, you can run
them on x86_64 Linux under `qemu-user`. That needs `sudo apt-get install
qemu-user clang` and `rustup target add aarch64-unknown-linux-musl`.

#### The Variables Cargo Needs

Set `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER` to `rust-lld`,
`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUNNER` to `qemu-aarch64`,
`CC_aarch64_unknown_linux_musl` to `clang` and
`CFLAGS_aarch64_unknown_linux_musl` to `--target=aarch64-linux-musl` in
the environment, then run `cargo test --locked --target
aarch64-unknown-linux-musl -p link-sandbox -p link-sandbox-confine -p
link-sandbox-proxy --lib`.

### Docs and Website Checks

`cargo test -p link-docs-check -p link-docs-check-code` checks a change
to the docs. The website's own checks, and
translating the docs, are on [The
Website](../../../web/docs/more/developing/website/README.md).

### No warnings on a Mac

Code that runs on only one OS sits under its `cfg`, so a build for the
other OS warns about whatever that leaves unused or unreachable. Clippy
for a Mac (`aarch64-apple-darwin`) catches those and must pass too, and
`checks.yml` runs it on a Mac.

#### Run it on Linux

Clippy compiles the Rust and links nothing, so it runs on Linux without
a Mac's SDK (Software Development Kit, the Mac's system headers). It
needs `rustup target add aarch64-apple-darwin`, `clang` and
`musl-tools`. The C that dependencies build (`ring`, `zstd` and SQLite)
is compiled against musl's headers as plain Unix code (`-U__APPLE__`)
and never run.

##### The Variables Clippy Needs

Set `CC_aarch64_apple_darwin` to `clang`, and
`CFLAGS_aarch64_apple_darwin` to `--target=arm64-apple-macos11
-U__APPLE__ -nostdinc -isystem <clang's resource folder>/include -isystem
/usr/include/x86_64-linux-musl`, where `clang -print-resource-dir` prints
that folder. Then run `cargo clippy --locked --workspace --all-targets
--target aarch64-apple-darwin -- -D warnings`.

#### Fix a warning at its cause

A warning there is fixed with the `cfg` the code it names needs, an
import moved under it, or the dead code deleted. Never
`allow(dead_code)` or `allow(unused)`.

## Ship it

A change goes on a branch of its own off `main`, and into `main` by a
pull request. Run `git checkout -b my-change origin/main`.
Try committed changes on a cloud machine with
[Developer Boxes](../../../scripts/docs/more/developing/setup/dev-boxes.md).
A release is a version bump merged into `main`, as [Branches and
Releases](../../../scripts/docs/more/developing/releases.md) says.

## More

The pages below hold the detail this one leaves out.

- [Architecture](../architecture/README.md): the core and its modules, and where new code goes
- [Conventions](setup/conventions.md): the rules every change follows
- [Writing Docs](../../../tests/docs/docs/more/developing/documentation/writing-docs.md): how a doc is written, where it goes,
  and its [limits](../../../tests/docs/docs/more/developing/documentation/writing-docs.md#limits)
- [Development Environment](setup/agents.md): what a machine needs to build and test Link
- [Testing](testing/README.md): how Local Link is tested, the two kinds of test, and the checks as one command
- [Testing Kit](../../../packages/test-kit/docs/more/developing/testing/testing-kit.md): the helpers every test uses, and what each replaces
- [Test Environments](../../../packages/test-kit/docs/more/developing/testing/test-environments.md): the rules that keep a test correct on every machine it runs on
- [Write a Module](modules.md): adding a module, harness, connection or cloud
- [Module Contracts](../modules/contracts.md): what each `lnk ... --json` prints
- [Specifications and Command Lines](modules/specs.md): adding a command's flags, or a specification, and its tests
- [Decisions](decisions.md): why it works this way
