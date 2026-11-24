# Sandbox Module

The sandbox module, `lnk-sandbox`: the one sandbox Link runs every
harness in, and anything else. `lnk sandbox run` and `check`,
and `wrap` for the modules that confine a program; its wall and its
proxy are libraries beside it.

<!-- docs-tree:start -->

## Docs

### Pages

- [Sandbox](docs/security/sandbox/README.md): The sandbox runs any program so that it changes only the folders you give it, reaches only what you allow, and never reads Local Link's settings
  - [Sandbox Guide](docs/security/sandbox/guide.md): Run any program so it changes only the folders you give it, reaches only what you allow, and never reads Link's settings
  - [Choose What the Sandbox Can Reach](docs/security/sandbox/reach.md): The programs a sandboxed program runs, the hosts, machines and models it reaches, the tools it calls and the pages it opens, each a flag of `lnk sandbox run` and a key of a specification
  - [Tools and Asking](docs/security/sandbox/tools/README.md): Tools run outside every sandbox, and the sandbox asks you before a tool's call or a page goes through
  - [Files](docs/security/sandbox/files.md): The files `lnk sandbox` keeps, and what each one holds
- [Sandbox Specification Guide](docs/config/specs/guides/sandbox.md): How Link reads a sandbox specification: what each section opens and refuses, how one OS's table and field flags change it, and what a file may never grant

### References

- [Sandbox Specification](docs/config/specs/sandbox.md): A sandbox, written down: each section one grant, and everything it leaves out closed
- [Sandbox Commands](docs/more/reference/modules/security/sandbox.md): Every command the `sandbox` module offers, from its declarations, in order of the resource each acts on

### Packages

- [Bridge](bridge/README.md): The package `link-sandbox-bridge` defines what every bridge out of an agent's sandbox is
- [Confinement](confine/README.md): The package `link-sandbox-confine` confines a program as tightly as the operating system allows while the program can still run
- [Proxy](proxy/README.md): The package `link-sandbox-proxy` is the sandbox's one way out, a filtering proxy that Local Link runs outside the sandbox, with a table of routes that names none of them
- [Sandbox Contract](contract/README.md): The package `link-sandbox-contract` holds the sandbox's policy, independent of how any sandbox implements it

### More

- [More](docs/security/sandbox/more/README.md): Every side that builds or calls a sandbox shares three boundaries
- [Decisions](docs/security/sandbox/decisions.md): Why the sandbox works the way it does
- [Security](docs/security/sandbox/security.md): What every sandbox guarantees, what can cross it, and how a specification is checked
  - [Features](docs/security/sandbox/security/features.md): Every feature of the sandbox, one row each, grouped by what it opens or closes
  - [Gaps and Limits](docs/security/sandbox/security/gaps.md): What the sandbox leaves open, what `check` reports as `open`, and where it can't run
  - [Sandbox Log](docs/security/sandbox/security/log/README.md): Every call a sandbox makes past its walls goes through its proxy, on a wire
  - [Sandbox Permissions](docs/security/sandbox/security/permission/README.md): A sandbox opens nothing of this machine until its specification or a flag asks: no files, no network, and no program beyond the system's own
- [Troubleshooting](docs/security/sandbox/troubleshooting.md): What to do when the sandbox can't start here, a program in it can't write, run or reach what it needs, or your agent's grants don't hold

<!-- docs-tree:end -->

## Code

- [`bridges/mod.rs`](src/bridges/mod.rs): the bridge for tools and pages
- [`check.rs`](src/check.rs): the sandbox tried from inside
- [`commands/mod.rs`](src/commands/mod.rs): `lnk sandbox`'s commands, one module each
- [`spec.rs`](src/spec.rs): a sandbox's specification
- [`wrap.rs`](src/wrap.rs): a program wrapped in its confinement
