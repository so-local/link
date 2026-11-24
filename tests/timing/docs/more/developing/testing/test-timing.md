# Checks and Timings

`lnk-check` runs a workspace's checks as one command, and its report
shows where the test time goes and whose each failure is. `cargo
full-check`, `cargo session-check` and `cargo test-copies` are its Cargo
aliases.

## Run the Checks

Run `cargo session-check -p <package>` while building a change, naming
each package it touched, and `cargo full-check` once per initiative.
Each step prints a line as it ends, and the command fails when any step
failed.

```text
full check of /home/me/link/link-public
fmt            ok          1.2s
clippy         ok        140.5s
build          ok        118.0s
test           FAILED   1290.4s  (exit 101)
shellcheck     skipped           lacks shellcheck
```

### Steps Are Data

The steps are the workspace's `.cargo/checks.toml`, in order, each a
command and what it needs. A step whose `programs`, `paths` or Rust
`targets` are missing is skipped, saying which, and `{packages}` stands
for `-p <name>` for each package named. Every step runs, even after one
fails. The one step marked `tests = true` is the test command.

```text
[[full]]
name = "shellcheck"
run = ["sh", "-c", "git ls-files '*.sh' | xargs shellcheck"]
programs = ["shellcheck"]
```

### What a Check Run Saves

Each step's output is in `target/check/<step>.log`, and a failed step
prints its last lines and its log's path. After the test step, the
report is rendered into `target/check/report/`, with every step's time
as its validation phases.

## Read the Report

`target/check/report/index.html` needs no server, scripts or network.
Its sibling `timings.json` keeps the measurement, each program's
outcome and the warnings, for a later comparison. Each duration comes
from a test program's libtest summary, which already includes its
concurrent tests, and the chart shows each program's share of their
summed time.

### Whose Each Failure Is

The report names each failing test with the first line it failed with,
and sorts the failures. One is the machine's when what it said matches
a signature in `tests/timing/signatures.toml`, such as a program that
isn't built: those collapse to one line per signature, with the fix.
One is a known flake when its package and test are in the workspace's
`flakes.toml`, or the base's when the base's run failed it too. Every
other failure is the change's own. Each group is listed and counted,
none is hidden, and Cargo's status is still the run's.

#### Each Group's Next Step

Each group ends with what to do about it, so whoever reads the
output knows which failures are its own and what a new flake needs.

### Copies of One Test

`cargo test-copies -p <package> <test>` builds the package's tests,
finds the program holding the test, and runs that one test as many
copies at once, `copies` in `checks.toml` (300). It prints how many
failed and keeps the first failure's output in `target/check/copies/`.
A flake shows under that load when the whole suite rarely meets it
twice. A fix is proved when the same number of copies all pass.

### Ranked Table

The table shows each area's test time, its share, and its passed,
skipped and failed tests. Skipped means ignored by libtest: a test that
returns early through a gate counts as passed. A program with no
summary has unknown counts and duration, never zero, and its row says
so. A module's nested packages join its group, while shared packages,
core and repository tests keep their own.

### Ownership

Compiler artifacts in the log map each test program to its package,
through `cargo metadata`. A plain text log identifies programs only by
unique target names, and ambiguous ones stay unassigned with a warning.

## Render a Saved Run

`lnk-test-timing` renders a run saved by hand, and runs no tests. Save
`cargo test --locked --workspace --no-fail-fast
--message-format=json-render-diagnostics`'s output, both streams, with
`cargo metadata --no-deps --format-version 1` beside it and Cargo's
status. Then run `cargo run --locked -p link-test-timing --bin
lnk-test-timing -- --log <log> --metadata <metadata> --status <status>
--output <folder>`.

### Compare Two Runs

`--baseline` with an earlier `timings.json` shows both durations and
their difference, and marks a group missing from either as a changed
selection. `--base` with the base's report sorts the failures it shares
as the base's, and `--flakes` names a workspace's `flakes.toml`.

### Keep Measurements Comparable

Compare runs on the same machine, thread count, selection and required
capabilities, with no competing build. `--revision`, `--machine` and
`--threads` record that context. `--phases` adds measured validation
phases in the `PhaseMeasurements` shape. Their elapsed time and summed
work stay apart, since phases can overlap.

<!-- json: link_test_timing::PhaseMeasurements -->

```json
{
  "schema": 1,
  "elapsed_seconds": 420.5,
  "cache_state": null,
  "phases": [
    {"name": "fmt", "wall_seconds": 1.0, "status": 0},
    {"name": "test", "wall_seconds": 126.0, "status": 101},
    {"name": "web", "wall_seconds": null, "status": null}
  ]
}
```

### Environment Exclusions

`--filtered-skip-reason` records why every filtered test in the run was
verified as an environment exclusion. Only then does the skipped count
include filtered tests. Use it only when every filter is one.

## Other Executors

Another executor adapts its saved case results to the
library's `Program` measurements and calls `measured` with
`MeasurementKind::Cases`, sharing the aggregation, report and renderer.
`single::read` reads one selected test's summary, telling a pass, a
failure and a skip apart. `Report::parse` reads saved reports and
baselines, and a comparison stays within one kind.

## What Timing Never Does

The report describes a run, and never changes its status: a failed test
command stays failed after its report renders. It never infers a cost by
subtracting one measurement from another, and never calls a build cold
unless its cache state was established.
