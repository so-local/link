# Testing

> Every Check in One Command

Local Link is tested by plain Cargo on your own machine, with no paid CI.
One command runs every check, and each test that starts a program runs
alone in a process of its own, so tests never meet through what they
share.

## What Runs

A check of the public workspace runs five kinds of check. Each kind
proves something the others can't, and each has its own page.

- **Unit tests** prove one part's logic: a parser, a state change or a
  calculation. They sit beside the code in each crate and run as threads
  of one process, in milliseconds.
- **System tests** prove what crosses a process: `lnk` and its modules,
  sandboxes, servers and agents, started for real from the programs
  `cargo build` made. Each runs in a process of its own.
- **Repository checks** read the whole repository: layers, dependencies, licenses, declared
  shapes, and the rules every test keeps.
- **Docs checks** read every page:
  links, headings, limits, and each command and specification key a page
  names, against the code that reads it.
- **Checks outside Rust** are `shellcheck` over every script and the
  website's own lint and build.

### Where Each Kind Is Explained

The [Testing Kit](../../../../packages/test-kit/docs/more/developing/testing/testing-kit.md)
holds what unit and system tests use. [Repository Checks](../../../../tests/repository/docs/more/developing/testing/repository-checks.md)
says each rule the repository is held to, and why. [Docs Checks](../../../../tests/docs/README.md)
holds the rules every page keeps.

## Check a Change

A session check proves a change while it is built, and a full check
proves a whole initiative before it merges into `dev`. Each is one
command, run from a workspace's root: `cargo session-check -p link-agent`
and `cargo full-check`. Both are plain Cargo.

### Session Check

`cargo session-check` runs the repository and docs checks first, since
they take seconds once built. Then it runs `cargo fmt`, Clippy over the
named packages, `cargo build` of the workspace's programs, and the named
packages' tests. Name each package the change touched with `-p`. Run it
as often as it helps.

### Full Check

`cargo full-check` runs the same steps over the whole workspace. It adds
the sandbox's unit tests on the release's C library (musl),
`shellcheck`, and the website's lint and build. It runs once per
initiative, on its integration branch, when the work is ready to merge.

### What the Command Prints

Each step prints one line as it ends: its name, and `ok`, `FAILED` or
`skipped` with what it lacked, such as a program that isn't installed.
Every step runs even after one fails, so one failure never hides
another. Each step's whole output is in `target/check/`, and the test
step's report sorts each failure as the change's own, a known flake's or
the machine's.

### Where the Steps Come From

The steps are data, in the workspace's `.cargo/checks.toml`, and the two
commands are Cargo aliases in `.cargo/config.toml` beside it. The runner,
`lnk-check`, and its report are [Checks and Timings](../../../../tests/timing/docs/more/developing/testing/test-timing.md).

## Unit and System Tests

Every program Cargo builds for `cargo test` declares its kind at its
root, `unit` or `system`. The kind decides whether its tests share a
process.

```rust
link_test_kit::program!(system);
```

### Why Two Kinds

libtest, Rust's test runner, runs a program's tests side by side as
threads of one process. Threads share memory, statics, open file
descriptors and the environment. A test that only computes shares
nothing that matters. A test that starts programs, opens sockets or
forks shares all of it, and most of the flakes this repository has met
came from that: one test's `fork` copied a lock another thread held, a
child inherited Cargo's build pipe, and one test's cache reset changed
another's count.

### Unit Programs

A unit program's tests run as libtest runs them, fast and side by side.
A unit test starts no program and opens no socket. A repository check
reads each unit program's tests and refuses one that does, naming the
line.

### System Programs

A system program runs each test in a process of its own. The testing
kit starts the test program again for each test, with a home, a temp
folder and a runtime folder made new for it. As many run at once as
libtest would run threads. When a test ends, everything it started ends
too: its whole process group, and on Linux every process still naming
its home. A test that hangs is ended at its deadline, five minutes by
default, naming itself and what it last printed. The run's output reads
as libtest's, one line a test and one summary, so every tool that reads
test output reads it unchanged.

### What Stays Shared

Two things stay the machine's, whatever the kind. Loopback ports and
display numbers are taken through the kit's held numbers, file locks
the kernel releases however a test ends. Permits for heavy work, such
as starting confined agents, are held the same way, half the machine's
CPUs at most, across every test program at once.

## Writing a Test

A test uses the [testing kit](../../../../packages/test-kit/docs/more/developing/testing/testing-kit.md),
`link-test-kit`, for its temp folders, the programs it writes and runs,
its waits, its ports and its skips. Clippy refuses the raw calls the
kit replaces. A test waits for the event it needs under a deadline,
never for a fixed time, and skips only through the kit's gates, when the
machine lacks a capability such as bubblewrap or root.

## Performance

A full check of the public workspace takes about 12 minutes on a 4-CPU
Linux machine with a warm build: formatting about 5 seconds, Clippy and
building the programs about a minute and a half each, and the tests
about 9 minutes.
Tests run 16 at a time rather than one per CPU, since most wait on
sockets, timers and the programs they start: the end-to-end program
takes 130 seconds at 4 threads and 76 at 16. Starting a system test's process
costs a few milliseconds. The report in `target/check/report/` shows each
package's share of the test time.

## Known Limits

These are the gaps a green run doesn't close, so a reader knows what it
proves.

- **A skip counts as a pass.** libtest has no skipped outcome, so a test
  that returns early through a gate is reported `ok`. The gate prints
  why it skipped, and `--show-output` shows it. A machine meant to run
  everything sets the `LNK_REQUIRE_*` variables, which turn skips into
  failures.
- **Fakes prove orchestration only.** Tests use stand-ins for the macOS
  Keychain, systemd, launchd, cloud accounts, paid models and payment
  providers. Passing one doesn't certify the real system.
- **The unit rule reads only a test's own lines.** A unit test that
  calls product code which starts a program isn't seen. System programs
  are safe either way.
- **Some tests need Linux.** Bubblewrap, Landlock and the leftover-process
  sweep are Linux's. On macOS a gate skips them, and system tests end
  only their own process group.
- **A flake can pass a run.** A flaky test that happens to pass says
  nothing, so known flakes are listed, and each is proved fixed before
  it leaves the list, as [Flakes](#flakes) says.

## Flakes

A flake is a test that fails and then passes on one build, with nothing
changed. Each one goes through the same life, so nobody has to remember
what to do with it, and the check says which step a failure is at.

### Sort Each Failure

`cargo session-check` and `cargo full-check` sort every failing test,
each group with its next step printed beneath it. A failure of the
machine's names its fix. One the base's run had too, or one in
`tests/repository/flakes.toml`, isn't the change's: report it and go
on. Every other failure is the change's own, to fix.

### Tell a New Flake

A failure sorted as the change's own, in a test the change didn't touch,
may be a new flake. `cargo test-copies -p <package> <test>` runs that
test as 300 copies, as many at once as the suite would run it, and counts
the failures. If any copy passes,
the test is a flake. If every copy fails, the failure is real, and it is
the base's when the base fails it too.

### Write Down a New Flake

A new flake in a test the change didn't write or touch goes in the
workspace's `flakes.toml`: its package, its whole test path, the day,
the commit, how it was run and the first line it failed with. A flake
is never retried into a pass or fixed on the way, and the change goes
on with its own work. A repository check refuses an entry missing
a fact.

### Fix and Prove a Flake

A flake in a test the change wrote or touched is its own, fixed before
the change is done. Listed flakes are fixed by whoever takes the list,
each at its cause, never by a longer timeout or a retry. A fix is proved
when the same number of copies that failed before all pass, and the
entry then leaves the list. A flake gets one attempt. One no attempt
proves is rewritten to prove its behavior at a cheaper layer, or
deleted where another test proves it.

### Remove the Cause, Not the Flake

A cause several flakes share is removed once, where every test meets
it: in the testing kit or in a rule a check holds. Running each system
test in a process of its own is one such fix, which ended a whole class
of flakes from tests sharing a process.

## Machines and Failures

Tests run on machines that differ in their user, kernel, paths, network
and load. [Test Environments](../../../../packages/test-kit/docs/more/developing/testing/test-environments.md)
says how a test stays correct on each, and what to do when one fails on
one machine and passes on another.
