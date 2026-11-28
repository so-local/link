# Test Environments

Tests run on machines that differ in their user, kernel, paths, network
and load. This page holds what keeps a test correct on every one, and
what a failure says when the machine, the checkout or the test is at
fault.

## Choose What a Machine Can Run

A machine has two axes: what its host allows (OS, kernel features,
privileges, an outer container's limits) and what it has installed.
Installing a program grants no capability, so bubblewrap installed in a
container whose host withholds user namespaces still can't run. Select
the checks the machine can prove, install a program only for the
component under test, and leave the rest skipped, saying why.

### Set a Machine Up

Probe each capability, install what the components under test need,
and set each `LNK_REQUIRE_*` variable the machine fully provides, as
the kit's [gates](testing-kit.md#gates) say. Run the suite once with
`--show-output`, read every `skipping, as it lacks` line, and either
provide what it names or leave its variable unset.

### Stop at an Explicit Skip

A gate reporting missing Landlock or another capability completes that
check for this machine. Never make a VM or change the host only to turn
skips into passes.

### Report What Ran

Report passed, failed and skipped from what was observed. libtest
counts a gated skip as passed, so coverage is the tests that ran, read
from the gates' lines, never the pass count. A suite not run has no
counts, and a failure an environment caused stays a failure.

## Record a Failure That Depends on the Machine

When a test passes on one machine and fails on another, record both
before changing anything: the OS and kernel, the user, whether
bubblewrap and Landlock work, the checkout's path, `TMPDIR` and the CPU
count, and how many runs of how many failed. Reproduce the input that
differs, fix the boundary that owns it, never the test's expectation,
and add the rule to this page in the same pull request.

## Users and Identity

Root reads every file whatever its mode, so a permission checked as
root proves nothing for an ordinary user: a root test checks as `nobody`
or a user of its own. A system user a root test makes is the machine's,
so each test names its users after itself and removes them however it
ends. A test killed mid-run leaves its user, and the next run refuses to
take it over: remove it with `userdel -r lnk-<name>`.

### User Namespaces

A command runner can wrap tests in a user namespace of its own. Link
refuses unsandboxed agents there, since it can't trust the machine's
identity, so an agent that refuses to start there is Link working.
Compare `/proc/self/uid_map` and the bubblewrap probe in the runner's
execution modes, and run with the capabilities the test needs rather
than weakening the check. Root in a container may create network
namespaces yet lack the `sysfs` mount entering one needs, which its
suite probes and skips through `gate::lacks`.

## Folders and Paths

Keep the checkout outside `/tmp`, which a sandbox specification can't
read, and a fixture a sandbox reads out of it too
(`TempDir::short_out_of_tmp`). A sandbox grants a fixture's folders,
never the folder holding them. Under `/tmp` every parent is readable,
so code that opens a granted folder through its parent passes there and
fails with `Permission denied` elsewhere. A test sees the difference with
`LNK_TEST_STATE_DIR` set.

### Trusted Ancestors

Protected settings need their files and every ancestor folder owned and
moded as production requires. A container with an untrusted root, or a
fixture under a writable `/tmp`, can't give that, so `gate::settings`
and `gate::trusted_file` skip. Folder-backed box tests need a whole
Linux UID map, and the environment fixture's `box_identity` gate skips
inside a user namespace.

### Paths That Differ

A Mac's temp folder is reached through a link, `/var` being
`/private/var`, so code compares resolved paths only with paths resolved
the same way. A rename across mounts returns `EXDEV`. A test's folder
may sit inside a checkout, so a test of what's found up to a
repository's root makes its own `.git`. A check that walks the checkout
lists files from Git, or skips `.agents`, `.claude` and `.codex`, where
coding tools keep worktrees.

## Programs and Processes

A fake reads its whole input before it answers, as the program it
stands in for does. One that answers and exits first leaves the asker
writing to a closed pipe, a `Broken pipe` only when the fake wins the
race. A fake that reports a process numbers it past 2^22, the most a
process can be, so code waiting on it never waits on a real one.

### Signals and Sessions

A process inherits the signals its parent ignores: a run under `nohup`
ignores SIGHUP, and one a script backgrounds ignores SIGINT, so a test
that signals a child sets them back to default first. A child that
needs its own session uses `ChildGroup::spawn_session`. A process `ps`
reports can already be a zombie, ended but unreaped, which signal zero
still finds, so `process::left_under` checks for running ones.

### Platform Differences

A Mac has no `setsid` program, and its `python3` can predate 3.11 and
`tomllib`, so a test needing it asks `gate::program` to import it. A
Mac's `/bin/sh` keeps a variable assigned before a function call after
the call. A Mac's `accept` gives a socket its listener's non-blocking
mode. The Linux release links musl, so the sandbox's unit tests run on
musl too.

### Toolchains

Rustup picks a toolchain by folder and by `RUSTUP_TOOLCHAIN`, which
Cargo sets for every test. A program a test runs in a sandbox, outside
the checkout, can get the machine's default toolchain, so its probe runs
as it will. `Unknown binary 'rust-analyzer'` means the default lacks
the component: `rustup default <pinned>` and `rustup component add
rust-analyzer`. A language-server fixture selects its toolchain before
entering the sandbox and puts that sysroot's programs first on `PATH`.

### Programs a Machine May Lack

Container images can lack OpenSSH's programs, so tests that sign a
release or make a signer skip through `gate::installed("ssh-keygen")`.
The Java and Kotlin language servers skip without `jdtls` and
`kotlin-language-server` on `PATH`, even under `LNK_REQUIRE_PROGRAMS`.
Node's `--test-isolation` arrived in Node 24, and a test using it probes
for it. A browser test finds Chromium on `PATH`, in `CHROMIUM` or under
`PLAYWRIGHT_BROWSERS_PATH`, and a desktop test needs `Xvfb`.

### Browsers in a Container

An installed browser also needs a working headless sandbox, so a probe
opens a fresh profile. On a machine that reaches the internet only
through an HTTPS proxy, a full Chromium hangs headless while `curl` gets through the same proxy, so `CHROMIUM` names
Playwright's headless shell instead. A launcher script around the
browser runs inside the agent's sandbox, which refuses what it calls,
unless the specification's `[tool.browser] program` names the engine.

## Network

A test never downloads. A machine may reach the internet only through
an HTTPS proxy, so a test gives every name it reaches an address
with `LNK_TEST_RESOLVE` and answers it itself, and one that speaks TLS
to its own host gives the proxy that host's authority with
`LNK_TEST_CA`. A request through a proxy doesn't prove the machine
resolves names or opens direct connections, so raw TCP and SOCKS checks
probe those apart.

### Fixture Rebuilds

The VM fixtures' committed manifests pin what a rebuild made, and their
tests read only the checkout. A rebuild fetches its recipe's archives
from `deb.debian.org`, so it runs on a machine that reaches that host,
and a manifest is never edited by hand. `cargo metadata --offline`
resolves every platform's dependencies, and fails with `--offline was
specified` until `cargo fetch --locked` has run once.

## Load and Timing

A failure only under load is a race in the code or the test, never
chance. Reproduce it by running the whole test program many times at
once on one build, then fix the race. Never add retries, a longer
timeout alone or one test thread to hide it.

### Order by Events

A test proves order by the events the code reports, never by elapsed
time: a loaded machine runs any step late. It waits until the first
step is seen before the next, a refusal for the refusal itself, and an
agent's health before timing its answer. A PID file publishes its whole
newline-terminated number, since creation precedes the write. A file
another process appends to is read under its lock, whole lines only.

### `waited ... for ...; the last check saw`

A wait ran past its deadline. Read what the last check saw: if the
awaited event never came, the code lost it, which is the bug. A machine
with few CPUs scales every wait by itself (a 4-CPU machine's waits are
three times as long, [`LNK_TEST_TIME_SCALE`](testing-kit.md#lnk_test_time_scale)),
and one slower than its CPUs say sets `LNK_TEST_TIME_SCALE` once.

### Clocks

A fake clock compared with a stored timestamp starts from the stored
value. A paused Tokio clock holds only the work that uses it: another
process's clock, such as a bridge child's typing indicator, keeps
running. A deadline of a second on a command also times the command's
start, which a loaded Mac can pass, so a test of a timeout makes only
the step it times hang.

### Ports

A port found free and bound later can be taken in between: `Address
already in use`. A test hands its server a listener it holds, and holds
a port a program takes itself with `HeldNumber` before probing it. A
port from the system's own range can be any port there, the sandbox
proxy's own among them, so a test's ports come from below it. A program
binding again after a run before meets `TIME_WAIT` unless it sets
`SO_REUSEADDR`.

## A Shared Lock Panicked

`panic in a destructor during cleanup`, then every test failing, means a
helper panicked while holding a lock other tests share. Every later
test panics on the poisoned lock, and the program aborts with its
programs still running. A shared helper lets its lock go before it panics,
and a drop never panics. In a system program each test's process holds
its own locks, so the poison stays in one test.

## The First Build Is Slow

A checkout's first `cargo` installs the pinned toolchain, then builds the
workspace from nothing, about two minutes for the programs on a 4-CPU
machine. An environment's setup script installs the toolchain ahead.

## What Local Fakes Don't Prove

Local tests use stand-ins for the macOS Keychain, TPM, systemd and
launchd, cloud accounts, paid models and payment providers. Passing a
fake doesn't certify those systems: they are checked on real machines
and accounts.
