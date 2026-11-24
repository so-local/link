# Development Environment

What a machine needs to build and test Link, whether a person or a
coding agent works on it. A coding agent starts from the workspace's
`AGENTS.md`, which points to these pages.

## Prepare the environment

A Linux checkout, outside `/tmp`, needs the Rust pinned by
`rust-toolchain.toml`, with rustfmt and clippy, a C compiler, Git,
curl, shellcheck, bubblewrap and OpenSSH's server; musl-tools and the
Rust musl target for the sandbox's release checks. JavaScript folders
need Node 20.9 or newer and their own lock file. If the image has a
read-only home directory, set `CARGO_HOME`, `RUSTUP_HOME` and npm's
cache to writable directories in the environment configuration. The
host supplies network access, its proxy and CA trust, and GitHub
authorization.

## What the sandbox's tests need

Link's sandbox on Linux stands on two things the machine provides:
bubblewrap and Landlock.

### Bubblewrap

Bubblewrap, with unprivileged user namespaces, makes every sandbox: its
own view of the files, network and processes. Without it, the
sandbox's tests skip, or fail with `LNK_REQUIRE_SANDBOX=1`. Install
bubblewrap, or use an environment that allows user namespaces.

### Landlock

Landlock (Linux 5.13 and newer, enabled) enforces finite executable
allowlists, including a specification without `[commands]`, which grants no
programs. Without it, executable enforcement checks skip, saying why;
unrestricted file and network checks, invalid specifications and
unsupported-policy refusal still run. Landlock is the kernel's: a
container shares its host kernel, so it can't add it.

### `LNK_REQUIRE_SANDBOX=1`

With `LNK_REQUIRE_SANDBOX=1`, tests fail if any required enforcement is
unavailable, including ruleset installation after a successful probe.
Never use a failing sandbox probe to claim a full test pass: this
variable makes missing sandbox coverage fail.

### What each test proves

Tests of file and network confinement use `allow = ["*"]` when they do
not check executable restrictions; this is still a confined process.
Executable tests require evidence from a running sandbox, since a
refused launch does not prove kernel enforcement. Full sandbox coverage
needs a Landlock-capable Linux machine or CI as well as an environment
without it.

### A client's own sandbox

If only the client's command sandbox blocks the probe, use that
client's supported permission mechanism for sandbox tests.

### OpenSSH's configuration

OpenSSH's `ssh` refuses a configuration (`~/.ssh/config`) owned by
another user or writable by others, so the tests that run it need the
image's to be the user's own and not writable by others.
