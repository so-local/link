# Developer Glossary

Names for the programs you run, the parts you install, and the Rust code
developers build. Link and Rust prefixes distinguish terms they share.

## Link Programs

A Link program is software you run through `lnk` to do a job, such as
managing agents or machines. Its commands have an entry point such as
[`lnk agent`](../../../modules/agents/agent/docs/agents/README.md#agents). A program remains available between runs; running it starts
an invocation with the configuration you chose.

## Link Modules

A Link module is what its `module.toml` declares: one executable,
`lnk-<name>`, that `lnk` installs, runs, updates and removes, supplying
a capability to the Link programs you compose. Choosing modules
determines what your programs can do.

## Contracts and Adapters

A Link contract specifies the operations, types, and behavior an
implementation must provide. A Link adapter implements that contract for
another product or service. The OpenClaw adapter and Hermes adapter
implement Link's harness contract so an agent can use either product.
An independently installable adapter is also a Link module. Executable
adapters use the same installation and discovery machinery as other
executable components. Adapter names its role, and module describes how
it is installed.

## Link Core

Link core is the shared machinery behind Link programs: the `lnk` command
and the library for installation, discovery, command routing, updates and
removal. The Agent program combines Link core, the Agent module, a harness
module and the other modules or adapters you choose. That composition
can involve several executables and processes.

## Link Packages

A Link package provides library code through dependencies. Programs link
that code instead of installing it as a separate capability. A contract
provided by a Link package is called a contract when its interface is
what matters. Link modules, adapters, and packages all use Rust's build units.

## Rust Terms

Rust describes how source code is organized and compiled. A Rust package
is defined by a Cargo manifest, `Cargo.toml`, and can contain library and
executable targets. Cargo is Rust's build and dependency manager.

| Term | Meaning |
|---|---|
| Rust package | Manifest-defined collection of Rust targets |
| Rust crate | Library or executable compilation unit |
| Rust module | Namespace and visibility boundary within a Rust crate |
| Rust workspace | Rust packages sharing Cargo workspace configuration |

## Link Components

A Link component is a Rust package represented in Link's repository
metadata. Repository checks, dependency analysis, documentation, and
release tooling read that metadata. A component can supply a module or
package, with roles such as adapter or contract. Executable components
additionally participate in runtime discovery; library metadata does not
make a library an executable or require it to answer runtime discovery requests.

## Programs and Processes

An executable is a program file, such as `lnk-agent`. A process is an
operating-system execution context with a process ID (PID). An invocation
executes in a process and can start more processes, including other
programs. Several processes can execute the same program. The programs
used together by a Link program can therefore run in separate processes.

### External Programs

An external program is software outside Link, such as Git or OpenClaw.
External describes its origin. Native describes machine-code software,
which includes Link's Rust executables, so it does not distinguish Link
programs from software made by others.

## Names in Prose

Use the most specific role when naming a part. Say OpenClaw adapter,
including when explaining its installation, and harness contract when
explaining the shared interface. Qualify overlapping developer terms
with Link or Rust. Installation categories and exact command spellings
do not replace the role's name in an explanation. Use adapter for a part
connecting Link to another product or service, and integration for the
activity of connecting systems.

| Classification | Preferred name |
|---|---|
| Adapter provided by a Link module | Adapter |
| Contract provided by a Link module | Contract |
| Contract provided by a Link package | Contract |
| Installable part that also supplies library code | Link module |
