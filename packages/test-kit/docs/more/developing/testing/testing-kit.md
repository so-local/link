# Testing Kit

`link-test-kit` holds what keeps a test free of races and of the
machine it runs on, written once. A test uses the kit's helper rather
than the raw call it replaces, and rather than a helper of its own, so a
fix lands in one place. A suite's own fixtures, such as a fake model,
stay with the suite.

## Use It

The kit is for tests only. A package names it as a development
dependency, and a crate that is itself only for tests, such as a
fixture, names it as an ordinary one. Code in a release never does.

```text
[dev-dependencies]
link-test-kit.workspace = true
```
## Declare the Program's Kind

Every program Cargo builds for `cargo test` holds `program!` at its
root: a crate's unit tests under `#[cfg(test)]` as the root's last item,
and each integration test program anywhere at its root. Its kind is
`unit` or `system`, and [Testing](../../../../../../docs/more/developing/testing/README.md#unit-and-system-tests)
says how to choose.

```rust
#[cfg(test)]
link_test_kit::program!(unit);
```

### A Home of Its Own

Link finds its folders, `~/.lnk` and `~/Link`, from `HOME`. Before the
program's `main`, while no thread can read the environment, `program!`
sets `HOME` to a folder the kit makes, whenever `HOME` is unset,
relative or the account's own (`home::account()`). The folder is short,
out of `/tmp`, empty at start and removed at exit. `XDG_RUNTIME_DIR`
becomes its `.run`, so the program's sockets are its own. `CARGO_HOME`
and `RUSTUP_HOME`, when unset, name the account's, so a `cargo` a test
starts finds the toolchain.

### Inherited Descriptors

Cargo hands a test program its jobserver's pipe. `program!` marks every
inherited descriptor past the standard three close-on-exec, so no
program a test starts holds the pipe, or anything else the run
inherited, for as long as it lives.

### System Programs

In a system program, `program!` runs each test in a process of its own
before libtest's `main` starts (`program::run`). It lists the tests with
`--list`, then starts the program again for each selected test with
`--exact <test>`, the variable `LNK_TEST_ONE` naming it, so that process
is ordinary libtest. Each process leads a process group of its own, with
a new home and `XDG_RUNTIME_DIR`.

#### libtest's Flags

libtest's flags work as usual: filters, `--exact`, `--skip`, `--ignored`, `--include-ignored`,
`--show-output`, `--nocapture`, `--test-threads` and `--list`. A flag the
runner doesn't run as libtest would, such as `--format json`, is
refused by name. A run of one test by its whole name, `--exact` and one
name, is that test alone in its process already, so libtest runs it
directly, as when a test runs its own program again as a fixture.

#### Its Deadline and Cleanup

`program.toml` holds the runner's numbers: a test's deadline (300
seconds, times `LNK_TEST_TIME_SCALE`), when a slow test is named (60
seconds) and how long a finished test's output is read. When the test's
process ends, or reaches its deadline, its group is killed, and on Linux
so is every process whose command line or environment names its home
(`process::end_under`). A passing test that left a process running says
so on its line.

#### Why a Constructor

`program!` is the kit's only code that runs without being called. It is
a constructor, as a C program's are, which the system's loader runs as
the program starts, so `cargo run` and every program in a Link release,
which never hold it, are untouched. A program the kit starts never
inherits `LNK_TEST_ONE`: `env::clean` leaves it out.
## Files and Programs

A test's folders and the programs it writes come from the kit, so each
is removed however the test ends and never runs into another test's.

### Temp Folders

`TempDir::new(name)`, instead of `std::env::temp_dir()`, `tempfile` or a
folder removed by hand, is removed when the test ends, failed or not,
and a killed test's folder goes on the next run. It uses the temp
folder's real path, because a Mac's goes through the `/var` link, which
SQLite refuses. `TempDir::under(parent, name)` makes one in a folder of
the test's choosing.

#### Short Folders for Sockets

A Unix socket's path has a limit, 104 bytes on macOS and 108 on Linux.
`TempDir::short(name)` keeps its path at most 44 bytes, under `/tmp`,
and a longer name fails the test. `TempDir::short_with_limit(name,
bytes)` gives a smaller budget to an owner such as SSH, whose control
socket adds a hash and a suffix.

#### Short Folders out of `/tmp`

`TempDir::short_out_of_tmp(name)` is for a fixture a sandbox reads that
holds sockets, such as an agent's home, since a sandbox refuses `/tmp`.
It uses a short `LNK_TEST_STATE_DIR` when set, else this user's cache
folder when a short path fits there, else `/tmp`, which
`gate::out_of_tmp` then reports. With `LNK_TEST_STATE_DIR`, compact
process numbers and counters keep suite folders distinct.
`TempDir::set_mode(mode)` changes a folder's mode for a test of an
unsafe ancestor.

#### Folders in Memory

`TempDir::in_memory(name)` is for a test that makes thousands of
durable writes, each synced to its disk, such as a store driven through
ten thousand turns. On Linux it's in `/dev/shm`, a file system held in
memory (`tmpfs`), where a sync returns at once: such a test takes
seconds there rather than a minute on a disk. Elsewhere, and with
`LNK_TEST_STATE_DIR` set, it's the folder `TempDir::new` makes.

### Programs

`write_program(path, text)` replaces `std::fs::write` and a mode change
before running the file, and `copy_program(from, to)` replaces
`std::fs::copy`. Both write in a child process, so no thread of the test
holds the file open for writing while another forks, which would make
running it fail with "Text file busy". They are `lnk`'s own
(`link_files::write_program`), so the code under test and a test write
programs alike. `link_program(from, to)` links a large built program a
test only runs, as a Mac checks each new copy on its first run.

#### Built Programs and Cargo

`programs()` names the `target/<profile>` the test was built in, where
`cargo build --workspace --bins` put `lnk` and its modules, and
`programs_in(root)` the same for another workspace. A test never builds
a workspace or downloads a tool, and `require(program, by)` fails with
one shared sentence naming the step that was missed. `cargo()` replaces
`Command::new("cargo")`: it removes the variables Cargo sets for a
running test, so a build the test starts sees a contributor's
environment and recompiles nothing.

#### A Stand-In `lnk`

`write_core(path, script)` writes a stand-in `lnk`, a `#!/bin/sh`
script. Before the script runs, it answers the core's history query, a
hidden flag that asks for the release's renamed and removed modules and
commands, with an empty history, as a program reading a specification
expects of its core.
## Processes

A process a test starts is owned by a guard that stops it, and gets an
environment of the test's choosing, never the machine user's.

### Process Groups

`ChildGroup::spawn(&mut command)` owns a background process in a group
of its own, after `env::command` sets its environment. Its drop stops
the group and reaps the leader, including on panic. The leader stays
unreaped until then, so its group number can't be reused. `take_stdin`,
`take_stdout` and `take_stderr` give its pipes for a fixture's
handshake, `interrupt` sends SIGINT, `terminate` SIGTERM, and
`try_status` observes an exit without reaping it.
`ChildGroup::spawn_session` starts a fixture in a session of its own,
without the runner's terminal.

#### Descendants and Reruns

On Linux, `left_under(folder)` lists every running process that names a
test's folder in its command line or environment, and `end_under` ends
them, for a fixture whose daemons start sessions of their own.
`process::Subreaper::for_fixture` lets a dedicated fixture process adopt
orphans where the container's main process doesn't reap them.
`process::rerun(test, home, log)` runs the test's own program again as a
fixture of `test`, its output in `log` and never in the run's.
### The Environment a Child Gets

`env::command(program, home)` replaces `Command::new` with a test's
`HOME`, and `env::clean(command, home)` does the same for a command made
another way. The child gets `HOME`, its `.tmp` as `TMPDIR`, its `.run` as
`XDG_RUNTIME_DIR`, `PATH`, the proxy and certificate variables
(`env::PASSED`) and the test's own `LNK_*`. It never gets one that holds
a secret, `LNK_TEST_ONE`, or the user's `USER`, `LOGNAME` or `XDG_*`: a
program that needs its user asks the system. Variables the test sets
afterwards are added to these.

#### The Keychain and Users

On a Mac, Link keeps secrets in the login Keychain through Apple's
`security` tool, and `LNK_SECURITY` names a program to run in its place.
`env::clean` points it at `keychain::stand_in(dir)`, which keeps each
item as a file, so no test writes your Keychain. `keychain::refusing(dir)`
fails every command, as a locked Keychain does. `users::root()`, `me()`
and `my_group()` replace `geteuid() == 0`, `id -un` and `id -gn`, and
`users::command_as` runs a program as another user.

#### Command Recording

A verification tool may set `LNK_TEST_RECORDER` to a TOML manifest
(`recording::Recorder`) naming a recorder program and the program names
it observes. `env::command` then runs the recorder with the original
program and its arguments, leaving program files unchanged. For a
fixture that product code starts itself, `recording::program(original,
folder)` returns a shim that hands to the recorder. An absent manifest
changes nothing, and a malformed one fails.
## Ports and Waits

Ports and numbers are held across every test on the machine, and a test
waits for what it needs under a deadline, never for a fixed time.

### Ports and Numbers

`Held::loopback()` replaces a port found free and bound later, which
another program can take in between: the test holds the listener and
hands it to the server, `.listener()` in its own process or
`.hand_to(command, variable)` to a child.

#### Numbers a Program Takes Itself

`HeldNumber::first(kind, candidates, free)` holds a number a program the
test starts takes itself, such as an X display, against every test
program on the machine: a file lock the kernel releases however the
test ends. `HeldNumber::port()` holds one loopback port from 12000 to
15999, below the system's own and Link's. A program that binds the
ports after its own is given a block, held whole by its base.

#### Permits

`permit::confined(n)` waits for `n` permits before a test starts `n`
confined programs, held to its end, so heavy starts don't outlast their
waits on a small machine. The machine has half its CPUs' worth, shared
by every test program through the same file locks, and a test waits for
its permits before any deadline of its own begins.
### Waiting

A test waits for the condition it needs, never for a fixed time.
`eventually(what, deadline, check)` polls a check that returns a
`bool`, an `Option` or a `Result`, from 5 milliseconds doubling to 200,
until it's done or the deadline passes. Past it, the test fails with
what it awaited and what the check last saw. `within` returns that as an
error instead, and the `_async` and `_await` forms do the same in an
async test.

```text
waited 10.0s for the provider's port file; the last check saw: absent
```

#### Deadlines and Polls

`Deadline::new(what, total)` bounds a whole test, cloned into its
threads, and its `output(command)` stops a program past it.
`wait::exited`, `wait::accepting`, `wait::accepting_unix` and
`wait::appears` replace polling `try_wait`, `connect` and `exists`.
`io::ObservedOutput` publishes how many bytes a writer has written, for
a test that interrupts a stream only once its output is shown.

#### `LNK_TEST_TIME_SCALE`

Every deadline is multiplied by `LNK_TEST_TIME_SCALE`, a positive number,
1 when unset. A machine slower than most sets it once, such as `3`,
rather than each test adding slack of its own. It never hides a race.
## Gates

A gate answers whether a test can run on this machine, and the test
returns where it can't. A gate is the only way a test skips. Where the
capability is missing it prints the test, what it lacks and why, which
`--show-output` shows.

```text
proxy::the_listening_proxy_refuses_this_machine: skipping, as it lacks bubblewrap: bwrap: No such file or directory
```

### One Gate a Capability

Each capability has one gate: `bubblewrap`, `landlock`,
`sandbox_with_landlock`, `sshd`, `root`, `out_of_tmp`, `program(name,
args)` for a host program, `installed(name)` for one with no command
that answers, and `sandbox` for the workspace's built `lnk-sandbox`. A
suite's own capability goes through `gate::lacks(what, why, variable)`.
A gate runs the capability as the test will, from the same folder and
environment, so it never passes where the test then fails.

### What Is a Failure Instead

A skip is the machine's, never the code's. A test never skips because
it is slow, flaky or broken. A workspace program that wasn't built is a
failure, since `cargo build --workspace --bins` comes first, and so is
a request the network refuses, since a test answers every name it
reaches through `LNK_TEST_RESOLVE`.

### Variables That Require a Capability

Each variable turns its capability's skips into failures. Set one only
on a machine that provides everything it names. A person's machine
leaves them unset, and the tests that need them skip, saying why.

- **`LNK_REQUIRE_SANDBOX`:** bubblewrap with unprivileged user
  namespaces and its own network, `sshd`, and test folders out of `/tmp`.
- **`LNK_REQUIRE_NATIVE`:** the kernel's Landlock, Linux 5.13 or newer,
  which a container can't add.
- **`LNK_REQUIRE_PROGRAMS`:** the host programs tests run, such as
  `python3`, `git`, `ssh`, `Xvfb`, `rust-analyzer` and a Chromium.
- **`LNK_REQUIRE_GENERATED`:** sources a step outside Cargo generates,
  such as a JavaScript workspace's modules.

### Root and Other Systems

Nothing requires root or another OS. A root-only test skips for an
ordinary user, and code only root runs is run as root before it merges.
A Linux-only test skips on macOS through its gate.
## More Helpers

These helpers check what a test's program printed, break what it reads,
or report for a test program with its own `main`.

### IDs in Output

`ids::assert_named(&json)` checks a command's `--json` for IDs written
outside their form, a bare one or `<kind>:<id>` under a field that
names no axis, and names where each is. `ids::assert_named_output`
does the same for one JSON value per line.
### Injected Failures

`fault::FileFault` removes or truncates one file below a test's
`TempDir` and restores nothing, so the test observes the product's own
recovery. `fault::Once` injects one error around an operation, before
it runs or after it succeeds, as a lost reply. A test calls
`assert_injected`, then checks the product's state and retries.
### Custom Test Programs

A test that needs `harness = false` uses `libtest::Selection` for
libtest's selection flags and `libtest::Counts::summary` for its final
summary, with actual outcomes and time. It reports cases not run after a
failure as ignored, with their reason.
## Clippy and Platforms

The kit's rules are held by Clippy and a repository check, and each
helper says which systems it runs on.

### Clippy and the Repository Check

Each workspace's `clippy.toml` refuses the raw calls the kit replaces:
`std::thread::sleep`, `tokio::time::sleep` and
`std::process::Command::new`. A call that stays says why, in an
`#[expect(clippy::disallowed_methods, reason = "...")]` where it is.
Patterns no one call shows, such as a skip printed by hand or a port
probed free, are read by [a repository check](../../../../../../tests/repository/docs/more/developing/testing/repository-checks.md#tests-keep-to-the-testing-kit).
### Platforms

Each helper has a Unix implementation. One with no Windows
implementation yet fails to compile on Windows, naming itself: system
programs, `process::kill_group`, `process::interrupt`,
`net::Held::hand_to` and `users`. `left_under` is Linux's, since macOS
shows a process's environment to no other process.
