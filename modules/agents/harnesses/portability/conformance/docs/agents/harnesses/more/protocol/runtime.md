# Runtime Comparisons

Compare Mini and full Link using built programs, local first-party model and
connection services, and Rust records shared with Measure. Confined cases include
protected tools, cold and warm turns, two agents, refusal and cleanup checks.

## Prepare built programs

Run from the repository root on Linux with Rust, bubblewrap and Landlock.
Choose a short writable fixture directory outside temporary folders. Children
receive isolated homes, with real services and the keychain off. The fixture
never builds or downloads programs itself.

### Build the Programs

Run `cargo build --locked --release -p link-cli -p link-agent -p link-sandbox -p link-measure -p link-model -p link-secret -p link-file-store -p link-mini-harness -p link-connection-fixture -p link-harness -p link-run-tool -p link-file-tool -p link-code-tool -p link-scheduler-tool`, then `cargo build --locked --release -p link-harness-conformance --example runtime`.

### Fixture location

Fixture state must be short and outside the sandbox's temporary folders,
since a long checkout path can exceed the Unix socket limit. Each run makes
its own child directory in `~/.cache` and removes it on completion.
`LNK_TEST_STATE_DIR` selects another short directory outside them, such
as a `~/lnk-tests` folder you make for it.

## Run the confined cases

The three cases run Mini, full Link and two full Link agents. They require the
actual kernel operations, rather than treating installed bubblewrap as proof.
Missing capability is reported as skipped, with no automatic unconfined
fallback. `LNK_REQUIRE_SANDBOX=1` and `LNK_REQUIRE_NATIVE=1` make their respective
missing capabilities fail on an environment contracted to provide them. Run `cargo test --locked --release -p link-harness --test harness runtime:: -- --nocapture`.

### What they exercise

Full Link calls the real run and file tools through their protected routes,
including a refused conversation write. Each agent keeps its files, two turns
complete, an unauthenticated endpoint is refused, and observed live processes
stop. A one-second monitor interval makes repeated sampling practical. The idle
phase waits for completed samples. Monitoring stops before the isolated status
series, so its queries cannot be mistaken for requested status work.

### Standalone cleanup

The standalone Rust runner also adopts and reaps orphaned fixture children
through the testing kit on Linux. It checks that none remain after the
domain owners stop their helpers. This keeps repeated diagnostics clean even
inside a container whose main process does not reap adopted children.

## Retain comparable observations

The Rust example writes JSON with source, artifact sizes and hashes, platform,
configuration, phase timings, roots and process identities. Run Mini before
full Link with the same installed programs. Read `outcome` and `coverage`:
successful diagnostics are not evidence of production confinement, root-secret
service behavior, real OS service managers, memory totals, or released bytes.

### Run the Diagnostics

Each run names the source it measures, the commit `git rev-parse HEAD`
prints. Run `target/release/examples/runtime run target/release
target/runtime-mini.json mini confined <commit>`, then the same with
`target/runtime-link.json link` in place of `target/runtime-mini.json
mini`.

### Separate tracing from timing

Use `strace` only for a separate controlled run. After it exits, the Rust
report mode counts successful images in each phase, including resumed execs.
Executing PIDs, execs and resident processes have different meanings. An empty
or malformed trace produces unavailable counts, rather than zero overhead.

### Trace a Run

Run the Mini case under `strace -f -ttt -e trace=process -o
target/runtime-mini.trace`, as `target/release/examples/runtime run
target/release target/runtime-mini-traced.json mini confined <commit> 1
traced`. Then `target/release/examples/runtime report
target/runtime-mini-traced.json target/runtime-mini.trace` counts it.

### Explicit unconfined diagnostics

The example accepts `diagnostic` in place of `confined` only when explicitly
selected. Protected run and file tools stay disabled in that profile. Full
Link exercises its scheduler instead. Keep this evidence separate from confined
qualification, and never substitute its process count for a secure deployment's.
