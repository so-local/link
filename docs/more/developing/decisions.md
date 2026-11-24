# Decisions

Why the code and its tests work the way they do. Why Link's specifications are what they are: [Specifications Decisions](../../config/specs/decisions.md).

## Tests

How tests keep apart, share a build and find their programs, and what
the conformance kit checks. How testing works as a whole is
[Testing](testing/README.md).

### Why do tests come in two kinds?

**Because a process is the boundary that keeps tests apart.** libtest
runs a program's tests as threads of one process, which share memory,
statics, descriptors and the environment. Most of the flakes this
repository wrote down came from that sharing, not from any one test: a
`fork` copying a lock another thread held, Cargo's pipe reaching a
child, one test's cache reset changing another's count.

#### One Boundary for a Whole Class

Each of those flakes was fixed once with a helper and a rule, and the
next was another. A system test runs in a process of its own, so the
whole class goes at once. Pure tests stay unit tests, since a process
each would cost them more than their work.

#### A Constructor, Not a Harness

The kit runs a system program's tests from a constructor that runs
before libtest's `main`, so a test is still a plain `#[test]`, `cargo
test` stays the command, and every tool that reads libtest's output
reads the runner's. A custom harness would need every test registered
by hand, and a wrapper around Cargo would break "plain Cargo with
nothing around it".

### Why does one command run the checks?

**Because a check assembled by hand is assembled wrong.** The full
check was four long commands, written out in the guide, the pull request
template and each session's skill, and each copy drifted. `cargo
full-check` and `cargo session-check` are Cargo aliases for one program
whose steps are data, so a step changes in one file and every caller
runs the change. Each step is still a plain Cargo command, printed in its
log.

### How do tests avoid "Text file busy"?

**By writing programs in a child process** (`link_test_kit::write_program`).
The test never holds a program open for writing while another thread
forks, so an inherited descriptor can't stop it running. Installation
writes to a temporary file and renames the finished program into place.

### Why does `cargo test` check the docs?

**Because a wrong command in a doc is worse than none**, and a developer
should see it before pushing. The docs checks fail on a broken link or
anchor, on an `lnk <group> <command>` no `module.toml` declares, and on
a `## Decisions` section. They read commands in code blocks and inline
code that starts with `lnk`, read the files as text, and need no built
`lnk`.

### Why do the folders beside the core share a build folder?

**They are members of one workspace.** One lockfile resolves the
dependencies, one profile turns off debug info, and one `target/` holds
all programs and compiled libraries. Members keep their own source,
tests and docs. The core is the default selection; connections and harness
adapters are selected only when a change reaches them. A separate
workspace can consume the public shared crates through sibling paths.

#### Each worktree its own target

Worktrees keep their own target directories: stable Cargo's freshness
checks use file times, so sharing a target directory between checkouts
can run another checkout's program. Each workspace owns one profile.
Debug information defaults to off; `CARGO_PROFILE_DEV_DEBUG=true` or
`--config 'profile.dev.package."<name>".debug=true'` enables it when
needed.

### Why do tests run 16 at a time, and poll?

Most tests wait on sockets, timers and child programs. The workspace sets
`RUST_TEST_THREADS=16` to keep those waits concurrent. A contributor's own
setting, or `--test-threads`, takes precedence. Tests poll the condition they need under a deadline. Fixed waits are used
only when time itself is the behavior under test, such as verifying that a
paused connection stays open.

### Why are a crate's tests one program?

A test crate can make its files modules of one `tests/main.rs`, with
`autotests = false` and one `[[test]]` target. Cargo links the dependencies
once, and the tests run concurrently in one process. Such tests use unique
temporary paths and do not change the process environment or current
directory. Tests that need independent process state keep separate targets.

### Why are some programs built with `test = false`?

A binary with no unit tests sets `test = false`, avoiding an empty test
executable.

### How does the conformance kit see what a harness keeps?

Through its own model. Every harness keeps its threads its own way, in
files the kit can't read, but every harness sends the thread to its
model at the next turn. So the kit's model records what it's sent: a
turn whose caller went before the answer is kept if the next message's
request holds that answer, and a tool's answer reached the model if the
model's last answer quotes it. The model finds the kit's message among
the harness's own (instructions, the date, a runtime's context) by its
text, not by its place, since harnesses add messages after it.

#### `TCP_NODELAY`

Whether the endpoint sets `TCP_NODELAY` isn't in anything it answers,
so the kit reads it from the harness's own socket: it opens a proof's
connection and, holding it open, finds the socket the harness accepted.
On Linux that's by `/proc/net/tcp` and the harness's descriptors, the
flag read through a copy of the descriptor (`pidfd_getfd`, which needs
the right to trace the harness, as its parent usually has); on macOS
from `lsof`'s TCP flags.

#### Where the system won't say

Where the system won't say (another platform,
an old kernel, no right to the descriptor), the kit says it didn't
check and goes on, rather than failing a harness for the machine it's
tested on.

#### Steering

The kit doesn't check steering. Steering is a harness's own choice, and
a harness that never steers never sends `Link-Steered`.

#### The kit's tool

The kit's tool is an MCP server of its own, given to the harness as Link
gives one (the settings' `tools`, a session's `mcpServers` through the
hub, and `$LNK_TOOL_KIT`), so what it asks
of a harness needs nothing new in the contract. A harness with no
endpoint, which speaks every connection itself, is checked only for its
contract's answers and its health. A harness that takes local models
only through Ollama says so to the kit (`Check::provider`). The kit's
`install` and `configure` get this machine's proxy settings, since a
real harness downloads.

## The mini harness and the tests beside it

Where the tests that need a harness, a tool or a temp folder live.

### Why is the mini harness called `lnk-mini-harness`?

Unix socket paths have a small length limit. The short fixture name leaves
room for the test's temporary folder and the sandbox's socket paths. The
mini harness is a test fixture and is never released.

### Where do the tests that need a harness or a tool live?

A harness's or tool's behavior is tested in its own package. Core tests use
the mini harness and test services, so they can run independently of any
installed harness or document extractor. Cross-package tests build the
programs they need through `link_test_kit::programs_in`.

### What does a test leave in the temp folder?

A test owns a `link_test_kit::TempDir`, removed on success or panic.
Its process ID and unique name keep concurrent tests apart. A test process
also removes folders left by processes that no longer exist. `TempDir::short` provides short Unix socket paths; `TempDir::under` keeps
files under a specific directory, including `CARGO_TARGET_TMPDIR`. CI checks
that no test folder remains after the suite.

## Builds

How Link builds, and why its parts build apart.

### Why does Link build without debug info?

Debug information increases build output and disk use. Each workspace
set `[profile.dev] debug = false` once at their roots. A panic still reports
its file and line; a backtrace names functions. Enable debugger information for a build with `CARGO_PROFILE_DEV_DEBUG=true`,
or for one package with `--config 'profile.dev.package."<name>".debug=true'`.

### Why doesn't each git worktree share one build folder?

Stable Cargo checks workspace files for freshness using their modification
times. Sharing a mutable build directory can therefore run a different
checkout's program. Cargo also releases its build lock before tests finish,
allowing another build to replace programs a test is running. Each checkout owns its target directory.

### Why do connections build apart from the core?

A connection library serves its connection module. It is selected only when
that module or a contract it depends on changes, keeping unrelated builds
free of its compilation and test costs. Release checks cover every module. Connection tests run their module against a fake service and an agent on
the mini harness. Core behavior that needs messages uses the mini
connection fixture, so a core test does not require a real service.

### Why does every folder lock one version of each crate?

**The workspace resolves one version of each compatible crate.** Its
members share a lockfile and the version is inherited from the
workspace. Dependabot updates the workspace as one group.
Upstream dependencies can require incompatible versions, such as
`syn` 2 and 3; those remain. Cargo resolves features
for the packages selected in each build, so a broader selection can
require another compilation of a shared dependency.

### Why is the build between dev and a release called staging?

A build kind says the environment it's made for. A staging build is
what runs before a release, for testing it: signed by its own
key, trusted only by builds of its kind, and versioned `-staging.<when>`
before the release it precedes. A version is read by semantic
versioning's rules alone, so no word in a prerelease is special to
`lnk`; only the build kind decides which keys it trusts.

### Why do document parsers run in a child process?

Link Memory's PDF and Word parsers are Link Document Converter,
`lnk-document-converter`, which memory runs as a child with no secrets or
inherited environment and with limits on memory, time and output. A
parser crash or malformed document cannot take down the memory service.
Parsing documents is a heavy capability few runs need, and a parser
reads what users hand it, so it's a module of its own: memory's build
compiles none of it, and a Link without it has no parser to attack.
Memory finds the program again for each document, so an upgrade of
either never leaves it reading with a stale one.

## Dependencies

Which dependencies Link takes, and how they stay trusted.

### Why only first-party or widely used dependencies?

Link runs where a leak is serious, and nobody audits a dependency line
by line, so whom a dependency comes from matters as much as our own
code. A vendor's own organization, or a package many others use and
watch, is what's trusted; an unofficial client of a service is
neither, and reaches it as its vendor never promised. The same rule
holds npm packages, development tools included: they run on
developers' machines and in CI. npm counts downloads by the week where
crates.io counts them in all, so its line is 100,000 a week.

### Why is the evidence a file, read again every day?

`cargo test` runs offline and fast, so the trust check reads a file
the workspace keeps, written from crates.io, npm's registry and OSV
when a lock file changes. A new advisory against a version already
locked, or an owner who changes, would then go unseen until the next
change, so a daily run reads them again and runs the same check.

### Why does an advisory fail even with no fix?

Every advisory fails until it's fixed or listed with why, and a listed
one fails once it no longer applies, so the list never outlives its
reason. Red that nobody can turn green teaches everyone to ignore red.
An unmaintained crate counts as an advisory: one that parses what
users hand Link is replaced, and only one that reads nothing from
outside may be listed.

## CI

What CI runs on, and what it pins.

### Where does CI's disk go?

A CI runner contains development tools beyond those Link needs. The
workflow removes large unused toolchains before building and reports disk
use even when a check fails. Compiled dependencies and test executables
are the main build outputs; tests remove their temporary files.

### Why does CI pin its Rust version?

A pinned toolchain gives contributors, checks and release builds the same
compiler and Clippy rules. Each workspace's `rust-toolchain.toml` and the
workflows' `RUST_TOOLCHAIN` agree. A toolchain
update changes them together and addresses the new compiler's diagnostics.

### What does CI cost, and why is it shaped this way?

The public checks run on pull requests into `main` and pushes to `main`.
Website checks run for their configured paths, and signed releases call
the reusable checks before building their bundle. Contributors also run
the documented checks locally so failures can be fixed before review.

### How do the release checks stay quick?

Release checks cover every member in one workspace job, sharing
its build outputs.
Every job has a timeout. The target-specific release builds run
concurrently, keeping one target from delaying the others. A Mac job only
builds the programs, because its minutes cost more and its tests need a
machine with no real agents.

### Why is Link's signer a GitHub secret?

The release workflow signs `SHA256SUMS` in a dedicated job using the
`bundle-link` environment's secret. Builds do not receive that secret,
and the signing job uses only first-party actions. Maintainers activate
the guarded workflow and configure [GitHub's required reviewer](../../../scripts/docs/more/developing/releases.md#only-behind-a-reviewer) and branch
restrictions.

### Why doesn't CI run on a machine of Link's own?

GitHub-hosted runners give each untrusted pull request an isolated machine.
No persistent project machine or its credentials are exposed to a fork's
code. macOS release targets run on Apple's hardware through hosted runners.

## What a release ships

Why a module a release holds back stays in the code, and how each part
of a release leaves it out.

### Why is a held module marked rather than moved?

A module a release holds back stays where it belongs, built and tested
with the rest, and dev and staging builds keep it. One mark in its
`Cargo.toml`, [`released = false`](../../../packages/component/docs/more/libraries/types/component-metadata.md#what-a-release-ships), is the one source the registry, the
archives, the website and the checks read, so launching it later is
deleting a line, never moving a tree.

### Why may a released module recommend a held one but not require it?

A recommendation is a choice a person makes, so a release drops the
ones it can't offer. A requirement installs a program the release
doesn't have, so a release's build refuses it, as the repository check
refuses a released crate linking a held one. A released module that
reaches a held one at run time asks for it by name and is told plainly
that the release doesn't have it.

### Why does the website read the mark from the documentation index?

The website reads other folders' docs and nothing else of theirs, so the
index carries the mark beside each folder, and a repository check holds
it to the crate's. A page placed only beneath a held one in the
navigation is held with it, since its section isn't the site's.

### Why does the core ask who owns what's made?

Boxes, buckets and agents are tagged with their owner, which every
release needs while only some have accounts. The core asks the question
(`lnk.account`) and an accounts module answers it, so the modules that
tag what they make link neither the accounts module nor its layer, and
with none installed the local owner owns what's made.

### Why does a module answer a question at its program's root?

A question crosses from one release's module to another's, so its wire
is versioned like any other between programs: the asking module
declares each question's protocol beside it, and both sides state and
check their version. [One hidden entry point](../../../core/framework/docs/more/modules/protocol.md#questions), `lnk-<module> answer
<question>`, run directly, keeps answering out of each module's command
tree and out of `lnk`'s routing, so a manifest lists only the questions
it answers and never a command to run.

## Link's own calls

Why `link-fetch` holds every call of Link's own is on [Fetch
Decisions](../../../packages/fetch/docs/more/libraries/runtime/fetch/decisions.md).
What's here is how a caller reads the answers of one route.

### Why does the report read its answer action by action?

Each action in a report is read independently, keeping a newer account
service compatible with an older watcher. An unknown action is refused
alone and reported in the next outcome, while recognized actions continue.
The service reads those outcomes independently too.

## Secrets on a Mac

Where a Mac's secrets live, and how Link reaches them.

### Where do secrets live on a Mac?

Secrets are stored in the login Keychain, keeping credentials out of
copies and backups of plain configuration files. Linux keeps protected
files. Callers hold secrets in the shared `Secret` type in memory.

### How does Link reach the Keychain?

**Apple's `security` tool,** not a Keychain library: an item made by `security` trusts it, and it's the same program across `lnk` upgrades, so reading never asks. Link's programs aren't signed by Apple, so items they made themselves would ask after every upgrade. Secrets go to it on stdin, hex-encoded, and are read back to check.

### How does rclone get secrets from the Keychain?

Each invocation receives `RCLONE_CONFIG_<SECTION>_<KEY>` variables, which
rclone reads as configuration entries. `rclone-keychain.toml` lists the
entries whose values are kept in the Keychain.

## The command line

How `lnk` teaches from the command line, and where its help lives. How its output looks, its colors, marks and layout, is [`link-format`'s decisions](../../../packages/format/docs/more/libraries/runtime/format/decisions.md).

### How do you tell which `lnk` runs?

`lnk --version` reports its version, program path, installation method and
other copies on `PATH`. Interactive commands show a note when multiple
copies are available. Module calls identify their invoking program through
`LNK_EXE`. The first version line remains machine-readable for upgrades.

### Why do `lnk`'s variables start with `LNK_`?

The prefix makes Link-specific inputs discoverable and consistent.
Commands also use declared system and provider variables, such as
`PATH`, `HOME`, and cloud credentials. The
[environment reference](../reference/environment.md) indexes those inputs.

### How do you find what `lnk` can do?

**On the command line, not only in the docs.** A person runs commands
and reads what they print; few read the docs first. So a command offers
what's next where it's needed: the first [`lnk agent start`](../../../modules/agents/agent/docs/agents/README.md#create-an-agent) asks which
of Link's tools the agent gets, asking for a key where one is needed,
rather than leaving memory to be found on a docs page; `status` lists
the tools it has and names [`lnk agent tool list`](../../../modules/agents/agent/docs/agents/README.md#add-tools-and-skills) for the rest; a hosted
model named with `--model.default` asks for its key rather than failing. Each
command's **Next** names the commands that follow from it, so [`lnk up`](../modules/install.md#pick-your-modules)
leads to everything.

### Why does a command's common form need no flag?

A person types the common form most, so what a verb acts on is its
positional and a flag is the strict form of the same argument:
[`lnk box create aws`](../../../modules/compute/box/docs/services/boxes/guide.md#create-a-box)
is `lnk box create --cloud aws`. The harness is set as the model is,
[`lnk agent harness set hermes`](../../../modules/agents/agent/docs/agents/harnesses/use/README.md#switch-in-one-command) beside [`lnk agent model set`](../../../modules/agents/agent/docs/agents/models/agent/README.md#switch-in-one-command), so one
pattern teaches both. Only a handful of such short forms exist, and a
flag stays wherever another argument already holds the positional, as
[`lnk box delete`](../../../modules/compute/box/docs/services/boxes/lifecycle/delete.md#what-deletion-does)'s box does.

#### Never a secret

No flag takes a secret's value, since a command line is visible to the
machine's other users. A hosted model reads its provider's variable,
such as `ANTHROPIC_API_KEY`, else the key the agent keeps, else asks
once and keeps it.

### Where does a command's help live?

Help belongs to the program that implements the command. The help reader
applies it to Clap only when help is requested or parsing fails, preserving
fast normal command execution. Paragraphs follow Clap's short and long help
rules. Tests check that every command and argument has help and that every
help entry names a real command or argument.

### Why do subjects own their contracts and documentation?

A caller links a subject’s public types and protocol without compiling its
implementation or engine. A test that executes that implementation names
its program separately. This keeps the core usable with an independent
service module and makes implementation changes reach subprocess callers.
A neutral package earns its location through reuse across subjects.

#### What each source owns

Cargo owns identity, versions, binaries, dependencies and short summaries.
Owner descriptors supply command help, typed requirements, installation,
removal and test inputs. Markdown supplies full pages. The generated
registry, help, website and optional offline manual consume those
sources. Canonical topic paths survive source moves through redirects.

### Why are external artifacts shared but build outputs isolated?

A digest identifies immutable downloaded bytes; the cache verifies them,
bounds retained size and gives each suite its own copy. Environment
identities include runtime and transitive inputs. Floating environments
remain explicitly untracked. Mutable services, generated files and test
state belong to one suite. A checkout owns its compiled output because
Cargo’s file-time freshness and live executables cannot safely be shared
with another checkout. Missing applicable capabilities report unavailable
coverage, and platform predicates report operations that do not apply.

## What makes an upgrade breaking

Removing or renaming a command, flag or `LNK_*` input, changing a JSON
reply or module protocol incompatibly, bumping a specification schema, or stopping
support for a published state format requires a major release. Additions
are minor releases and corrections that preserve the contract are patches.
Specification transformation chains keep published input formats readable. Retired
command names belong in their module's `history.toml` rather than
parser aliases, and renamed modules in the core's.
