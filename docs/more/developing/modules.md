# Write a Module

> Extend Link with a Module

Add a module, a harness, a memory engine, a connection or a cloud to Link,
with its code in the module it belongs to.

## How a module runs

Why `lnk` is a core that runs modules, and the rules that follow, are in
[The core and its modules](../architecture/guide.md#the-core-and-its-modules).
Mechanically, [`lnk box create ...`](../../../modules/compute/box/docs/services/boxes/guide.md#create-a-box) runs `lnk-box create ...`
from `lnk`'s own folder, or else the PATH, with `LNK_EXE` set to the
core. Modules use each other only by running `lnk <command>`
(`link_core::lnk()`) and reading a documented output, and [Module
Contracts](../modules/contracts.md) lists every one.
Changing an output changes an interface, so keep old fields and add new
ones.

## Where code goes: the seam test

Code belongs in the module whose purpose it serves, and a module's
purpose is its name. `box` makes machines in your clouds, `signal` talks to
Signal, and a harness's adapter runs that harness. Before adding code to
module P, ask:

1. **Would I still use this code without installing the module it
   serves?** If code in P serves Q's purpose, everyone who installs P
   carries Q's code. If Q needs it, installing Q drags in P, a module the
   user may not want. Then the code goes in Q, or in a module of its own
   when several modules need it and none owns it.
2. **Does P have to know another module's files, API or quirks?** Then P
   breaks when that module changes, and can't work with one from outside
   Link. P asks the other module through [a documented `lnk <command>
   --json`](../modules/contracts.md) instead, or the other
   module hands P what it needs as data.
3. **Would a developer look for it here?** "Where does code for X and Y
   go?" must have one answer.

The same holds for the shared library (`link-core`), which every module
links, so nothing in it serves one module. It also holds for the core,
which knows modules only by name, what they need, and how to run them.

### An outside harness and Signal, worked through

Say a harness from
outside Link speaks Signal itself. The code setting up its own Signal
runs only for that harness, and only its adapter knows it, so it goes in
that adapter, in its own repository. The Signal module carries messages
for any harness that speaks no Signal, and never names a harness. So
code for "that harness and Signal" goes in its adapter, and code for
"Signal for any harness" in the Signal module. Code that knows Signal
but sits in neither, such as the harness module or the sandbox, fails
the test.

## A command's help

Each program declares its commands in its `actions.toml`, and the
shapes they take in `types.toml` beside it. Cargo supplies the short
package summary, `module.toml` declares installation and operational
requirements, and Markdown owns full explanations. The program's
parser, its specification kinds and its dispatch are generated from these
declarations, so help and [`lnk variable list`](../../../core/cli/docs/more/architecture/files.md#list-a-commands-variables)
read what the program parses. Environment inputs are listed apart from
command arguments and flags.

```toml
# actions.toml
schema = 1

[actions."agent.agent.create"]
description = """
Make another agent, and start it (--agent <name> then picks it)

The first paragraph is the short help; the rest shows with --help.
"""
role = "primary"
output = "core.report"
spec = "agent"
docs = "/agents/start/create#create-on-a-hosting-account"

[actions."agent.agent.create".args]
name = { type = "string", description = "Its name", required = true, x-link-cli = { position = 1 } }
yes = { x-link-cli = { shared = "yes" } }
```

An action is named by its ID, `<namespace>.<resource>.<verb>`, and its
command comes from that ID. The
[Catalog](../../../packages/actions/docs/more/developing/modules/catalog.md)
explains every file and what is checked.

### Specification binding

An action that reads a specification names its kind once, at the action, with
`spec = "<kind>"`. [The kind's declared shape](../../../packages/spec/docs/more/libraries/types/specifications.md#specification-shape) is then the action's list
of specification fields, each a field flag holding a dot, so discovery and agent
tools read the binding and the shape rather than a field per argument.
An action binds at most one kind, and a binding to a kind no module
declares is refused.

### Runtime arguments

An action's `args` are its runtime arguments, what this run acts on or
how it answers, positionals among them. They never hold a dot, which
keeps them apart from field flags, and each is explained in its
command's section.

### Shared flags

A flag many modules take is declared once, with its help, in
`link-actions`' `shared.toml`: `json`, `yes`, `quiet`, `relay`,
`request` and `actions` among them. An action takes one by naming it,
`yes = { x-link-cli = { shared = "yes" } }`, and the generated parser
gives it that declaration's type and help, so `--json` and `--yes`
mean the same in every command by construction. The action's own
description of a shared flag is only what it adds after the shared
help, such as what `--json` prints.

#### Counted shared flags

`--cloud`, `--owner`, `--account` and `--remote` are taken once,
required or repeated. Their types in `link-component`'s
`shared.toml` are `counted`: a command declares the flag as its own
argument, typed as the type says, and the flags check holds every
declaration to the meaning the type declares.

#### Composed types and their positionals

A type marked `composed` adds a group of flags to every command naming
it: `workplace` is [`link-workplace`](../../../packages/workplace/docs/more/libraries/types/workplace.md)'s
`--on`, `--cloud` and `--here`. A composed type may also declare a
positional under `positionals`, which every command composing it takes
after its own positionals, its help declared once: workplace's cloud
positional, short for `--cloud <cloud>`, with `here` short for `--here`
(`lnk box create aws`).

### Roles and sections

Every action says what it is to its module with `role`:

- `primary`: what gives the module its function, the tasks its pages
  feature first, such as [`lnk box create`](../../../modules/compute/box/docs/services/boxes/guide.md#create-a-box).
- `secondary`: what supports a primary one, such as [`lnk box show`](../../../modules/compute/box/docs/services/boxes/guide.md#show-one-box).
- `plumbing`: what Link runs and a person doesn't, such as a bridge one
  program starts for another. Its parser hides it, and help never
  offers it.

A primary or secondary command names the section explaining it, by its
page's website address and the heading's anchor: `docs =
"/services/boxes/guide#make-a-box"`. The address is what a reader's
link uses, so it holds when a page moves between owners, and a command
whose tool explains it, such as [`lnk agent browser`](../../../modules/agents/tools/browser-tool/docs/agents/tools/browser/guide.md#terminal-actions), names the tool's page. The
docs checks resolve it, want that section to show `lnk <command>`, and
want a primary command's section to be a heading of its own: below its
page's title, and no other primary command's of its module. Plumbing
names no section.

### Specification first

Whatever a flag sets, a specification can say, so an action that reads a specification
binds its kind, whose fields are the action's specification fields, and its own
arguments are runtime arguments. Additional derived settings belong to
the operation's resolver, rather than being a second set of CLI
options.

## Adding a module

A module is an owner-local Cargo member with one `lnk-<id>` binary and
an adjacent `module.toml`.

1. Choose its subject home, add the member to the workspace, and use
   `link-<id>` as its Cargo identity. Shared local dependency paths
   belong in `workspace.dependencies`.
2. Declare its `id`, kind, menu, removal and requirements in `module.toml`,
   its commands in `actions.toml`, and the shapes they take in
   `types.toml`. Its build generates its parser and handler trait from
   them.
3. Embed its descriptor and owned pages with `link-component-build`.
   Generic registry, routing, releases and help consume that
   declaration; no core routing or release list is edited.
4. Put cross-owner wire types and clients in the owner's `contract/`.
   Compile access goes through contracts; executing a program declares its
   command separately, including typed options and platform predicates.
5. Keep adapter unit tests independent of operational engine installation.
6. Write the component's pages in its own `docs`, including security
   behavior and decisions. Add them to `scripts/documentation-index.json`,
   then run the docs checks.

See [Components](../../../packages/component/docs/more/libraries/types/component-metadata.md) for the closed schemas and
[Module Protocol](../../../core/framework/docs/more/modules/protocol.md) for installed
answers and removal behavior.

## Adding a harness

A harness is a module of its own, in its own repository. It is an
`lnk-<name>` program answering [the harness program
commands](../../../modules/agents/harnesses/portability/contract/docs/agents/harnesses/more/protocol/README.md#harness-programs-lnk-name)
its contract declares in its `actions.toml`, from `contract`, `install`
and `configure` to `state show` and `conversations list`, with
`endpoint` for one that speaks no connection. The contract is
`link_harness_contract::harness`, an owner-local contract dependency, and
its adapter's own `actions.toml` names the commands it implements.

### The harnesses Link comes with

Link comes with Link Harness, and builds and releases the OpenClaw,
Hermes and DeepSeek Harness adapters too (`lnk-openclaw`, `lnk-hermes`,
`lnk-deepseek`), each at the core's version. The personal agents come first,
and each adapter's page says what it supports. Their descriptors declare
the harness kind, command protocol, requirements and installer records.
The core's registry derives from those declarations.

### Extend the choices

Contributors can add a harness or improve an existing adapter through
these same contracts. [Harness Adapters](../../../interop/agents/harnesses/docs/agents/harnesses/adapters/README.md)
introduces the available adapters and their scope. The
[Agent Protocol](../../../modules/agents/harnesses/portability/contract/docs/agents/harnesses/more/agent-protocol.md)
explains shared control and portable state before the command reference.
Harness creators can implement its protocols directly.

### Declare state capabilities

Declare portable export and import separately, using the Agent State
Protocol (ASP) kinds the adapter can actually read and restore.
The native `state` inventory names files for machine moves, while
`traces` identifies existing installations and their resources.
Those answers serve different operations. Common creation and control
remain useful when a portable kind is unsupported, and the conformance
kit checks the adapter's declared interface.

### One that speaks the Agent Client Protocol

A harness that speaks the Agent Client Protocol (ACP) needs no connection code. Its `endpoint`
names where, and Link carries the connections to it, as
`lnk-harness` shows; one that speaks it over stdio is served there
by the harness hub, `link-harness-hub`. The endpoint answers `GET /_link/proof` with
`link_harness_contract::harness::endpoint_proof`, and the contract says `proof`, so
Link's connections send its key only to a connection that proves it's the
harness.

### Its rules

An adapter follows these rules:

- It keeps mutable configuration and native state under
  `link_files::home_dir()`, with `HOME` set to the agent's private
  harness home. Installed programs use the adapter's supported program
  layout. The guide names those locations and the state inventory
  excludes programs and regenerated credentials.
- It installs the harness with the harness's own installer, pinned to a
  release with `link_harness_contract::harness::run_installer`. That runs the script
  from the project's repository at the release's commit, checked against
  its SHA-256, and the adapters' `VERSION` comments say how to move to a
  new release.
- It points what its installer's tools download into the staged program
  folder, the one folder an install may run programs from, with variables
  it gives the script through `run_installer_with`.
- It configures the harness with its own `config set`,
  never with a secret on its command line, since a command's arguments
  are any user's to see: a value that holds one (a tool's or a local
  model's address, which holds its proxy's) goes in the harness's
  `.env` with `merge_env_file`, named in its settings by variable where
  the harness reads `${VAR}`, or into its settings file itself,
  mode 600.
- It answers `uninstall --parts --json` with what its harness keeps in
  each agent's home (`link_harness_contract::harness::homes`): a credential its
  harness made, or a login, as the `agents` part's `credentials`, for
  [`lnk secret list`](../../../modules/security/secret/contract/docs/config/secrets/more/contract.md#listed-for-lnk-secret-list).
- It puts secrets in the harness's own files with `merge_env_file`, never
  in `Launch.env`, which ends up in a service definition other local users
  can read.
- It answers `contract` with `link_harness_contract::harness::Contract::new(proxy)`,
  and `proxy` is true only when a test shows the harness reaches its model
  at `model.base_url`.
- It is released from its own repository, signed with its own key, and
  users add it with [`lnk mod add --url <its URL>`](../modules/guide.md#add-or-remove-a-module), as [adapters from outside
  Link](../../../modules/agents/harnesses/portability/contract/docs/agents/harnesses/more/protocol/README.md#adapters-from-outside-link)
  says.

### Its tests

`modules/agents/agent/tests/up.rs` drives a fake adapter script.
Every harness's tests also run the conformance kit
(`modules/agents/harnesses/portability/conformance`, `link_harness_conformance::check`), which runs
its adapter as `lnk-agent` would and checks its endpoint.

#### What the kit checks

It checks that its `contract` speaks this Link's version (2, whose `needs` are
specifications), the key and proof, that the connection answering the proof has
`TCP_NODELAY`, that threads are kept apart, that a turn whose caller
goes is still kept, and a call to the kit's own tool through the bridge.
A harness that takes local models only through Ollama is checked with
`Check::new(adapter).provider("ollama")`.

#### Who runs it

The core's own fixture harness, `lnk-mini-harness` in
`modules/agents/harnesses/mini`, passes the kit, and the core's
end-to-end tests run on it. Link Harness's package runs the kit, and
each adapter's runs it.
An outside adapter runs the same kit against its own executable and the
workspace programs its tests require.

### Its sandbox

A harness needs nothing of its own for the sandbox. The harness module
runs every harness in the same one, with its home, `Launch.write`,
`Launch.read` and its agent's grants. What it needs beyond Link's
defaults, such as `[network] mode = "host"` as a sandbox's specification, or no
sandbox at all, it says in `needs`, with why, and the user is asked
before it runs. Change the harness to say it, never to fit the sandbox.

## Adding a memory engine

A memory engine is a module that serves an MCP server for an agent's
folder. Link's is in `modules/agents/tools/memory-tool`, and one from
outside Link lives in its own repository. It answers the commands its
contract, `link_memory_tool_contract`, declares in `actions.toml`:
`install`, `launch show <folder> <state>` and `serve-many`, each
engine listing those it implements, and its output is described in
[Memory Engines](../../../modules/agents/tools/memory-tool/contract/docs/agents/tools/memory/more/protocol.md#memory-engines-lnk-engine).

### `launch show`

`launch show` prints how to serve an MCP server over stdio for the folder,
keeping its index or database in `<state>`, which is also its `HOME`. The
engine runs confined. It reads the folder and its own program and nothing
beside them, and it writes only `<state>`, so an engine that installs
anything puts it there. Three settings shape the rest:

- Name the calls that only read in `allow`, so they don't ask.
- Set `models` only if the engine calls a model on this machine.
- Name an embedding model in `model` to have Link serve it (`lnk model
  serve`) outside the sandbox while the engine runs, at
  `$LNK_MEMORY_MODEL`, which is empty when no runtime here has it.

Then add a line to `MEMORIES` in the harness settings, with the name you
type, its module and what it is. For a module that comes with Link, declare its kind and operational
requirements in `module.toml`. `lnk-memory-tool`, in
`modules/agents/tools/memory-tool`, is the example. Its `tests/agent.rs` drives it through
[`lnk agent memory start`](../../../modules/agents/tools/memory-tool/docs/agents/tools/memory/README.md#memory-start) and searches it from the agent's sandbox.

## Adding a connection

A connection is a module that lets people text an agent, through `lnk agent
connect <connection>`. It lives in a folder of its own beside the core,
`interop/agents/connections/<connection>/`, a package in the workspace, as
`lnk-<connection>`, built on `link_connection_contract` (and `link-core`, by path), and its `main` takes the connection's `About`. [A core
build never compiles it](decisions.md#why-do-connections-build-apart-from-the-core).
It answers three commands:

- `connect` asks for what the connection needs, refuses a bot another agent
  has (`check_taken`), pairs the owner with a one-time code
  (`pairing_code`), and prints the connection's settings and its bot. The
  bot's `id` is what another agent's `connect` refuses. `lnk-telegram`,
  `lnk-slack` and `lnk-discord` are the examples.
- `describe --json` gives its title, which of its settings are secret (a
  Mac keeps them in the Keychain), its hosts, and what disconnecting
  leaves. The harness module keeps this with the connection, so it names no
  connection.
- `rule --json` says how a harness speaking the connection reaches the
  service with a placeholder while the proxy adds its secret outside the
  sandbox, as `lnk-telegram` does. It is `null` when the harness holds the
  secret.

The connection is the harness's own when its adapter speaks it. Each adapter
that does lists it in `connections` and writes it in `configure`, reading its
section of `link_harness_contract::harness::Settings`, which is under its name in
`connections` for a connection from outside Link. For a harness with an
`endpoint`, the module carries the messages too, with `bridge`,
`main_with_bridge` and `ask`.

### A linked account

A connection whose account is linked as a device also answers `link` and
`state` (`main_linked`), as `lnk-signal` does. Each
adapter that speaks it answers `link`, unless the connection's module
installs a program into the harness's home and links it there (`cli
install`, `cli link`, and `cli` in `describe`). `lnk-signal` does that
with `signal-cli` for harnesses that speak Signal.

### Its tests

Its own tests run in its folder: unit tests of what it reads and
sends, and connection tests such as Telegram's `tests/connection/bridge.rs`.
Those tests use the workspace's `lnk` with the conformance kit's agent on the mini
harness (`link_harness_conformance::agent::programs_beside`) against a
fake of its service. What the core does whichever connection carries the
messages is tested in the core, on the connection fixture
(`tests/fixtures/connection`).

### In the workspace

Add the member to the workspace, declare its connection kind and
requirements in `module.toml`, and the commands it implements of its
contract in `actions.toml`. The release registry consumes
that declaration. It inherits the workspace version; the workspace's CI and
Dependabot cover it. A connection from outside Link needs only to be on
the PATH.

## Adding a cloud

A cloud is a module, an owner-local module member, that answers [the adapter
commands](../../../modules/cloud/cloud/contract/docs/services/cloud/more/adapters.md)
its contract, `link_cloud_contract`, declares in `actions.toml`: for
machines `launch`, `wait`, `start`, `stop` and `remove` among them, for
images `image-save`, `image-list` and `image-remove`, and for buckets
`storage`, `buckets`, `bucket-credential` and `revoke-key`. The adapter
commands page lists every one.

### What a cloud owns

A cloud owns everything about itself, which is its CLI or API, its names
and its sign-in. The account it describes is the one sign-in every module
acts as, boxes through its CLI and buckets through `storage`. So
`lnk-cloud`, `lnk-box` and `lnk-storage` never know which cloud they're on.
Pass the account's keys to the cloud's CLI in its environment, never on a
command line, after removing the shell's own settings for that cloud.

### Its descriptor and tests

Declare its cloud kind and requirements in `module.toml`, and the
contract commands it implements in `actions.toml`. Release and runtime
discovery use that descriptor. `lnk-aws` and `lnk-gcp` are the examples,
and the tests in their `tests/aws/` and `tests/gcp/` drive a fake CLI.

### A cloud that hosts agents

A cloud that hosts agents itself, with no box, answers the [hosting
commands](../../../modules/agents/agent/contract/place/docs/agents/more/interface/contract/hosting.md)
too. [`lnk agent move`](../../../modules/agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud) then moves an agent
to the account and back in the move's two phases, [`lnk storage push`](../../../modules/storage/storage/docs/services/storage/guide.md#back-up)
and `pull` reach its Home there, and [`lnk agent chat send`](../../../modules/agents/agent/docs/agents/chat/README.md#talk-to-it-here) talks to it there.

#### What the adapter owns

The adapter owns the transport, its
service's API signed in as the account, and `lnk` only pipes the move's
streams (`link_agent_contract::moving`) to and from it. What it answers is
untrusted: every name, ID, hash and time is checked before it's used or
shown. One that doesn't answer the hosting commands hosts none. The host fixture,
`tests/fixtures/agent-place`, answers every one of them from a folder
for the core's tests, encrypting nothing, and is never released.

## More about it

[Decisions](decisions.md) says why it works this way, and [Module
Contracts](../modules/contracts.md) lists every module's interface.

## Typed Inputs

[Specifications and Command Lines](modules/specs.md) explains how configuration fields and runtime arguments enter through the
same typed declaration, so callers and help describe the values a module
actually reads.
