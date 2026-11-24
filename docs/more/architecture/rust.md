# Built in Rust

**Every program in Local Link is written in Rust:** `lnk` and its
modules, Link Harness, Link's tools and the harness adapters.
Each comes as a native executable for macOS or Linux, without requiring
a separate language runtime.

## Native Executables

A release builds every program for Apple Silicon and Intel Macs, and for
Linux on x86-64 and arm64. Linux releases use musl, avoiding a
distribution-specific C runtime. `install.sh` downloads `lnk` alone, and `lnk` adds each
module as one more file beside it. Link itself needs no Node, Python or
other language runtime on your machine. Feature-specific tools and
operating-system capabilities still apply, as the
[installation guide](../modules/install.md#what-you-need) and each
component's setup guide explain. A harness adapter manages the external
harness's runtime prerequisites and removal.

## Safe code where it handles your secrets

The sandbox and secret bridges enforce which requests can use model
credentials and bot tokens. Safe Rust rules out whole kinds of bugs,
such as reading past the end of a buffer or using memory after it's
freed, before that code runs. What the sandbox
guarantees, and its known gaps:
[Security](security.md).

## Light enough to run all day

Your agent's proxy, its bridge to your connections and its tools run beside
it for as long as it does, so they're small and quick to start.
[Measure](../../../modules/tooling/measure/docs/tooling/measure/README.md) shows what each agent and
this machine use.

## One definition for flags, files and tests

Every command has a typed invocation. Reusable configuration belongs
to a registered [specification](../../config/README.md), whose type supplies both
TOML reading and field flags. Runtime arguments choose the resource,
output, or operation and stay out of that reusable specification. Help and
contract checks read the same declarations.

## The code

The packages share one Cargo workspace, built with
the one Rust version pinned in `rust-toolchain.toml`. Where code goes and
what may depend on what: [Architecture](README.md). How to
build, test and ship a change: [Developing](../developing/README.md).
