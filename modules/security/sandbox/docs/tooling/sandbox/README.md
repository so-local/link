# Sandbox

> Sandbox Any Program to Only What You Allow

The sandbox runs any program so that it changes only the folders you give
it, reaches only what you allow, and never reads Local Link's settings.
Link Agents use the sandbox by default, and you can use it for your own
commands. A [Sandbox Specification](../../config/specs/sandbox.md) says what the
sandbox allows, as a file or as flags.

<!-- quickstart -->

## Check the Sandbox on This Machine

Check whether the sandbox can run on this machine, and install any support
it needs. [Machine setup](guide.md#make-the-sandbox-work-on-this-machine)
lists the requirements for each platform.

```sh
lnk sandbox check --ready
```

## Run a Program in the Sandbox

Run a program with only the folders you give it, or write all of its
permissions down in a file. [Running a program](guide.md#run-a-program)
covers the complete workflow. For agents, your grants combine with the
access a harness asks for, as [Permissions](../../../../../agents/agent/docs/tooling/sandbox/permissions/README.md)
explains.

```sh
lnk sandbox run --commands.toolchains shell --files.write . -- bash
```

## Test What the Sandbox Blocks

Check from inside the sandbox that disallowed access is blocked. The
check uses the same permissions as [a run](guide.md#run-a-program).

```sh
lnk sandbox check --files.write .
```

## Answer a Sandbox Request

Approve or deny a pending sandbox request with the code the sandbox prints.
Replace `k7m2pq` with that code. Each [approval surface](guide.md#answer-what-a-sandbox-asks)
works the same way.

```sh
lnk sandbox answer k7m2pq yes
```

## Choose What a Program Can Reach

[Choose What the Sandbox Can Reach](reach.md) names every permission group.
[Features](security/features.md) compares what the sandbox can open and
close, and [Tools and Asking](tools/README.md) covers the tools that run
outside the sandbox and the owner's approval. [Security](security.md)
states what every sandbox guarantees, and [Gaps and Limits](security/gaps.md)
lists what stays open.

## Find Saved State and Fix Failures

[Files](files.md) locates saved policies and runtime
records. [Troubleshooting](troubleshooting.md) fixes the common failures,
and [Decisions](decisions.md) explains why the sandbox works as it does.
A box controller confines remote logins with [sandboxed SSH sessions](more/sessions.md)
and refreshes its launcher through [worker controller maintenance](more/workers.md).

## Build on the Sandbox

The sandbox confines each program with [the operating system's own tools](../../../confine/docs/tooling/sandbox/wall/README.md)
and filters its network traffic through [a proxy that runs outside the sandbox](../../../proxy/docs/tooling/sandbox/more/proxy/README.md).
Callers and implementers share four boundaries: the [Sandbox Contract](../../../contract/docs/tooling/sandbox/more/contract.md),
the [Client Interface](../../../contract/client/docs/tooling/sandbox/more/contract/client.md),
[Bridges](../../../bridge/docs/tooling/sandbox/more/bridges.md), and
[Routes](../../../contract/route/docs/tooling/sandbox/more/contract/routes.md).
[More](more/README.md) gathers these references in one place.

<!-- docs-tree:start -->

## Docs

### Pages

- [Sandbox Guide](guide.md): Run any program so it changes only the folders you give it, reaches only what you allow, and never reads Link's settings
- [Choose What the Sandbox Can Reach](reach.md): The programs a sandboxed program runs, the hosts, machines and models it reaches, the tools it calls and the pages it opens, each a flag of `lnk sandbox run` and a key of a specification
- [Tools and Asking](tools/README.md): Tools run outside every sandbox, and the sandbox asks you before a tool's call or a page goes through
- [Files](files.md): The files `lnk sandbox` keeps, and what each one holds

### More

- [More](more/README.md): Every side that builds or calls a sandbox shares three boundaries
- [Decisions](decisions.md): Why the sandbox works the way it does
- [Security](security.md): What every sandbox guarantees, what can cross it, and how a specification is checked
  - [Features](security/features.md): Every feature of the sandbox, one row each, grouped by what it opens or closes
  - [Gaps and Limits](security/gaps.md): What the sandbox leaves open, what `check` reports as `open`, and where it can't run
  - [Sandbox Log](security/log/README.md): Every call a sandbox makes past its walls goes through its proxy, on a wire
  - [Sandbox Permissions](security/permission/README.md): A sandbox opens nothing of this machine until its specification or a flag asks: no files, no network, and no program beyond the system's own
- [Troubleshooting](troubleshooting.md): What to do when the sandbox can't start here, a program in it can't write, run or reach what it needs, or your agent's grants don't hold

<!-- docs-tree:end -->
