# Components

> Components Declare the Programs Link Runs

A component is a program `lnk` installs and runs on its own: a module,
a harness program, a connection, a tool, a store driver or a standalone
program. `link-component` is the one typed description of a component
that its build, the installed program and `lnk` all share.

## What a component is

A component is one Cargo package with one program, `lnk-<id>`, and a
`module.toml` beside it. Its kind says which interface it answers:
`module`, `harness`, `connection`, `tool`, `store` or `program`. A
library or a contract isn't a component, since programs link it and
nothing installs it.

## Where its description comes from

Cargo gives a component its identity: its id, Cargo name, version,
license, description and program, the `Component` type. Its
`module.toml` gives its kind, what it requires and recommends, its
place in [`lnk up`](../../../../../../docs/more/modules/install.md#pick-your-modules)'s menu, how it's removed, and its documentation, read
into `Source`. Its `actions.toml` gives its actions, each an `Action`
keyed by its command line: its help, its arguments, whether it's
hidden, plumbing between programs a person never runs, and where it's
offered (`Surface`), on the CLI, as an agent's command, a route or a
tool. A field either type doesn't know is refused.

### What a hidden command is

A hidden command, or a command under a hidden group, is Link's and not a
person's. Link runs it, such as a backend's typed request on standard
input, and never offers it. The registry carries it as `Command.hidden`,
so `lnk --help` does not list its group and Next's graph does not count
it. Routing still dispatches it.

### Command Importance

Each action declares its role: `primary` gives the module its function,
`secondary` supports a primary command, and `plumbing` is hidden work
between programs. Primary commands have an owning documentation section;
plumbing is documented only under the owner's More references. Command
depth groups subcommands, `dispatch` assigns a command prefix to a module,
and `surfaces` selects where it is offered.

### What each command leads to

An action's `next` and `fail` are the Nexts its command can give on
success and on failure, each a command as `cmd!` writes it (`lnk agent
connect <connection>`), `retry` for itself again: Next's graph, read by
format's check.

### What each command diagnoses

An action's `diagnoses` is the kind of name (`agent`, `box`) its command
diagnoses: given one, `<command> <name> --json` answers that name's
state as typed blocks, the same a person sees, and [`lnk issue create`](../../../../../../core/cli/docs/tooling/issue.md#report-a-bug) puts it
in an issue scrubbed. Each kind has one; an end-to-end test holds the
contract, with the kinds no command diagnoses yet on a list that may
shrink, never grow.

## What an installed program answers

At build, `link-component-build` joins the two into `Resolved`, the
self-contained answer embedded in the program. When `lnk` asks a
program what it is, this is what it gets back, so help, routing and the
install menu work without installing anything the program needs.

## What an operation needs

`requirements` works out what one command needs installed, from the
`requires` of every component it reaches: a module it runs, with the
command and options it runs it with, or a capability of the machine.
The machine's OS, architecture and libc, and the operation's options,
decide which apply. A recommendation installs nothing until it's
chosen, and a cycle of required commands is refused.

### Values the module refuses

An option given a value its type or listed values refuse, on the
command line or through its environment variable, binds nothing
(`Values::Refused`). Then `lnk` installs and offers nothing for the
command, and the module's own parser refuses it with its usage code.

## Documentation

A component's `[documentation]` names its topic, where [`lnk man`](../../../../../../modules/tooling/manual/docs/tooling/manual/README.md#manual) and
the website find its pages, and says with `quickstart = false` that its
first page has no quickstart, for a program only Link runs. An
installed module carries its own pages, and one that isn't installed
shows where to get it.

### Build-Time Embedding

[Build-Time Embedding](../../../../build/docs/more/libraries/types/component-metadata/build.md) explains the build-script entry points, embedded pages,
registry generation, and the inputs refused before a component is packaged.

## What a release ships

Every workspace member ships. The release checks use Cargo's
program inventory for archives and documentation. Each component declared
with `module.toml` also appears in the registry.

### Optional release exclusions

The component format supports holding a member back with
`released = false` under `[package.metadata.link]`. A held member
stays in the code and is built and tested, and dev, staging and source
builds keep it. Only a production build's registry and archives, and the
website, leave it out. No member sets it today. Any other value of `released`, `true` among them, is refused.

### The registry of a production build

A production build's `lnk`, Link's public release among them, is built
with `LNK_BUILD=prod`, and
`link_component_build::registry` then embeds only the released
components (`link_component::release::registry`). A released one's
recommendations of a held one are dropped, but a requirement of one,
or an upgrade that adds one, fails the build.

### Its other parts

[`lnk bundle`](../../../../../../modules/tooling/bundle/docs/tooling/bundle/README.md#bundle) packages and seals the same selection,
and the website leaves out each folder `scripts/documentation-index.json`
marks `"released": false`. A repository check refuses a normal or build
dependency of a released crate on a held one, and holds the index's
marks to the crates'.

## Question answers

A module's `answers` lists the questions of modules below it that it
answers, each by its name (`<module>.<what>`, `check_question`):

```toml
# module.toml
schema = 1
answers = ["storage.folders", "storage.owner"]

[module]
id = "example"
kind = "module"
```

It answers them all through one hidden flag at its program's root,
`lnk-<module> --link-answer <question>`, which its generated parser
has. A name that isn't a question's, or a question listed twice, is
refused. How a
question is asked and answered is the
[framework's](../../../../../../core/framework/docs/more/modules/protocol.md#questions).

## Account setup

`account_services` names exact HTTPS origins whose successful sign-in
sets up this component and its required modules. Only declarations
embedded in the trusted core registry select components. A service
cannot supply a program, download URL or capability. A component with
account setup declares the questions it answers in `answers`; discovery
then reads the cached login through that existing typed question.

## Action routes

An action offered as a route or tool names its callers and bridge,
exactly one of `answer` and `carries`, and positive `limits.bytes` and
`limits.time_ms`. The registry refuses incomplete definitions.

### Caller discovery

The bridge establishes `owner`, `agent`, `program` or `trigger` from its
trusted context. Registry discovery filters actions by caller and surface;
request arguments cannot change the caller. Canonical routes use
`/_link/<namespace>/<action>`; encoded and relative paths do not match.

### Answer budgets

A bridge applies each action's byte and time budgets before returning a
typed answer. `streams` distinguishes a stream from a single answer;
`carries` names a protocol relayed without a Link answer type.

## Command Tools

An action can expose its existing command through MCP by listing the `tool` surface and the `agent` caller, setting `bridge` to `command`, and declaring `answer` as `Json` or `Text`. It declares positive `limits.bytes` and `limits.time_ms` values. JSON commands must declare `--json`. A hidden, streaming or protocol-carrying action cannot use this bridge.

### Read Classification

`read_only = true` marks a command whose calls need no change approval. Omission means a change. This classification controls the Link agent's sandbox allowlist, so a command with a mutating option must be treated as a change. The installed descriptor retains this policy with its generated tool metadata.

### Tool Inputs

Tool names and schemas are projected from declared commands and their typed arguments. Transport and approval flags are caller-owned, and an agent-bound server withholds the agent selector. A value-taking flag with `optional_value = true` accepts JSON null for its bare spelling and a typed value otherwise. Other command validation stays with the owning parser, so commands and tools share the same implementation.
