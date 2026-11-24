# Architecture Guide

**`lnk` is a small core that runs a module for each part of Local Link,
and every agent runs in Link's sandbox, which lets it touch only what you
allow.** This page is how the code is laid out and where new code goes,
and [Decisions](../developing/decisions.md) say why it's built this way.

## The core and its modules

A user installs the core alone with `install.sh`, and [`lnk up`](../modules/install.md#pick-your-modules) asks which
[modules](../modules/README.md) to add.

- A module is its own program, `lnk-<name>`, next to `lnk`, and `lnk
  <group> ...` runs it, the way `git` runs `git-<name>`. What a user
  installs is the executable that runs, without linking every other
  module into it.
- The core only installs, runs, updates and removes modules. New behavior
  goes in a module, never the core.
- The core knows each installed program by what it describes through
  the versioned [module protocol](../../../core/framework/docs/more/modules/protocol.md):
  its commands, kind, requirements and documentation. The core's own
  settings: [Files and Variables](../../../core/cli/docs/more/architecture/files.md).
- The command line routes an operation to the program that declares it,
  the release registry supplies installation choices and upgrade
  dependencies, and removal combines the state declarations installed
  programs return.
- Modules use each other only by running `lnk <command>` and [reading a
  documented output](../modules/contracts.md). They never
  link each other's code or read each other's files.
- Each of Link's modules is released as its own signed archive at the core's
  version. So are Link Harness, Link's tools (Link Memory) and the
  OpenClaw, Hermes, and DeepSeek Harness adapters,
  built from their own members. A harness from outside Link is released from its own
  repository, signed by its own key.
- **Code goes in the module whose purpose it serves**, and a module's
  purpose is its name. Before adding code to a module, ask whether you
  would still use this code without installing the module it serves, and
  whether this module has to know another's files, API or quirks. If
  either is yes, the code goes in the module it serves.
- **A capability several modules need has one owner.** Callers use
  that provider's commands or shared contract rather than copy its
  behavior. The test worked through is [Where code goes: the seam
  test](../developing/modules.md#where-code-goes-the-seam-test).

## Layout

[Repository Organization](organization.md) owns the folder tree,
component ownership and layer declarations. Keep placement decisions
there, while this guide describes the interfaces and runtime behavior.

### Kinds

The [Developer Glossary](glossary.md) defines Link roles and Rust
build units. [Repository Organization](organization.md#kinds) explains
how those roles appear in source folders and generated README headings.

## Dependency rules

What a crate may depend on, and where what it declares lives:

- Cross-owner business logic uses the provider's owner-local contract.
  A contract's transitive graph contains no implementation or fixture.
  Generic core code and neutral libraries contain no domain contracts.
- Standalone composition declares exact importing modules and optional
  hosted features in Cargo metadata, rather than being a blanket
  exception for an application.
- The core links generic protocol and neutral packages. Its registry,
  routing, installation menu and removal behavior derive from descriptors.
- Neutral top-level packages have multiple subjects using them or a core
  consumer. Helpers owned by one subject stay beneath that owner.
- Cargo owns identity, binaries, versions and compile dependencies.
  `module.toml` owns operational requirements, `actions.toml` owns
  commands, runtime arguments and their help, and `types.toml` owns
  every shape, specification kinds among them. Rust is generated from these
  declarations, and Markdown owns authored explanations.
- Runtime command requirements are distinct from compile dependencies.
  Executing another program names its command and typed option bindings;
  those requirements propagate on the executing platform.
- Tests never provision an engine just because the adapter can install
  it. Every checkout owns its build output.
- A released crate never builds with one a release holds back
  (`released = false`), though its tests may.

See [Components](../../../packages/component/docs/more/libraries/types/component-metadata.md) for the
schemas and commands. A new crate goes in the workspace's
`members`; shared local paths are declared once in `workspace.dependencies`.

## Where does new code go?

Prefer a new module over growing the core or another module, and keep
crates small and names literal. [Adding a
module](../developing/modules.md#adding-a-module) covers how, and each
list below says, for what you're adding, the folder it goes in.

### Account API Client

The [API Client](../../../packages/api/docs/more/libraries/runtime/api-client.md)
owns the typed client and schemas for account operations. A route or
field starts in `packages/api/schemas/account.yaml`, then its client
function in `packages/api` (`link-api`). Types are generated from the
schema at every build. The client holds no credential. Its caller
hands it one.

### Models and harnesses

The models an agent runs on, and the harnesses that run it:

- **Finding, serving or managing models:** `modules/agents/model`.
- **Installing, configuring, running or switching harnesses:**
  `modules/agents/agent`.
- **Link Harness** (its turns, its conversations, its window, calling
  tools): `modules/agents/harnesses/link/`, beside the core in the
  workspace, with its pages
  (`modules/agents/harnesses/link/docs/agents/link/`). The
  core's tests run on the mini harness (`modules/agents/harnesses/mini`),
  and every harness passes the conformance kit
  (`modules/agents/harnesses/portability/conformance`).
- **What the adapter contract asks of every harness:**
  `modules/agents/harnesses/portability/contract` (`link_harness_contract::harness`),
  answered by Mini Harness and checked by the conformance kit.

#### Support for one harness

Support for a harness from outside Link is its own repository, not this
one: an `lnk-<name>` answering the adapter commands in
`link_harness_contract::harness` (with `contract`), released and signed
with its own key, and added with [`lnk mod add --url <url>`](../modules/guide.md#add-or-remove-a-module). It runs
sandboxed with HOME set to the harness's home folder, so it writes
everything under `link_files::home_dir()`. What is specific to it is
[its adapter's answer through the contract](../developing/modules.md#adding-a-harness).

#### The harnesses Link comes with

The adapters Link comes with (OpenClaw, Hermes, and DeepSeek Harness) are
named in `KNOWN` (harness settings) and nowhere else under
`src/` (`architecture.rs`).

### An agent's moves, routes and connections

Where an agent's traffic leaves from, how it moves between machines,
and the connections people text it on.

#### Where an agent's traffic leaves from

Where an agent's traffic leaves from, and the filtering proxy: `modules/agents/agent` decides which upstream
(`settings.upstream`, `moving.rs`). The sandbox module holds the proxy
(`proxy.rs`): a sandbox in its own network, with Link's proxy as its
only way out.

#### Moving an agent between machines

Moving an agent, with [`lnk agent move`](../../../modules/agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud), is `modules/agents/agent`
(`moving.rs`, `transfer.rs`, `hosted.rs`), plus each harness adapter's
`state`; the stream's format, each file's hash and what the other end
holds (`have`) are in `link_agent_contract::moving`. Two phases: a copy
while the agent answers, then a switch of what changed, the agent
paused (`lnk agent pause`).

#### How a move travels

One machine's `lnk agent state-send` is piped into the other's `receive` over
SSH, through [`lnk box run`](../../../modules/compute/box/docs/services/boxes/ssh.md#run-lnk-on-a-box), or into a cloud account's adapter when it hosts
agents (`agent-receive`, `agent-send` in `link_cloud_contract`).

#### Support for one connection

Support for one connection (Telegram, Slack, Discord, ...), where you
talk to your agent, is a new module in a folder of its own beside the
core, `interop/agents/connections/<connection>/` (`lnk-<connection>`),
answering `connect`, `describe` and `rule` in
`link_connection_contract`, which make the bot, pair its owner and say
what the harness keeps of it. Declare its kind in `module.toml` and
update each adapter that speaks it. The harness module names no
[connection](../developing/modules.md#adding-a-connection).

#### A connection in the core's tests

A core build [compiles and tests no connection](../developing/decisions.md#why-do-connections-build-apart-from-the-core):
the core's own tests that need one run the connection fixture
(`tests/fixtures/connection`, `lnk-connection-fixture`) against its fake
service (`link_harness_conformance::fixture_connection`).

### Tools, memory and stores

The tools an agent calls, its memory, and where its conversations are
kept:

- **A tool for agents** (an MCP server any harness calls), or Link
  Memory (what it indexes, its search, its MCP tools):
  `modules/agents/tools/`, with pages beside each tool. `lnk-memory-tool
  serve <folder>` is an MCP server over stdio that reads only its folder
  and never follows a link out of it.
- **[Support for one memory engine](../developing/modules.md#adding-a-memory-engine):** a module of its own (Link's is in
  `modules/agents/tools/`) answering `install`, `launch show` and
  `serve-many`, which `link_memory_tool_contract` declares. Declare its kind and requirements in
  `module.toml`.
- **Where Link Harness keeps conversations,** a store driver (SQLite, a
  database off the machine): a module of its own answering `contract`
  and `call` in `link_store_contract::store`, checked with
  `link_store_contract::store::check` (and `check_jobs`, for one that
  keeps a scheduler's jobs too).
- **Link's store drivers:** `modules/agents/stores/sqlite-store` (`lnk-sqlite-store`),
  declared as a store module. An agent's specification names it (`[storage] conversations`).

### The sandbox and its bridges

What one harness may do (its folders, ports, which permissions it has)
stays in `modules/agents/agent`. Anything else that should run confined
writes a `Spec`, makes a `Confinement` from it (`Confinement::of`) and
calls `link_sandbox_client::wrap`.

#### What a sandbox can and can't reach

The Seatbelt and bubblewrap rules, permissions, the probes (`lnk
sandbox check`) and `lnk sandbox run` are `modules/security/sandbox`;
the rules themselves, per OS, are in its wall
`modules/security/sandbox/confine` (`linux/`, `macos/`). The policy and
permissions a caller passes are `link_sandbox_client`.

#### A bridge out of an agent's sandbox

A bridge is a program outside the sandbox doing one kind of thing for
the agent. It goes in the module that runs it, on the server every
bridge runs on (`link_sandbox_bridge::serve`), saying only what it does with a
call and within what limits; its tests run
`link_sandbox_bridge::conformance::check` against it, listening. What every
bridge is, who's calling, its proof, the error envelope, the server and
that test, is `modules/security/sandbox/bridge` (`link-sandbox-bridge`).

### Measures, files and clouds

What's measured, the files sent to the user's clouds, and the cloud
accounts and boxes:

- **Measuring agents and this machine** ([`lnk measure`](../../../modules/tooling/measure/docs/tooling/measure/README.md#measure): what's read
  from outside, the watcher, what it keeps, the export):
  `modules/tooling/measure`. The measures and how they cross programs,
  OTLP, are `link_measure_contract::client`. What a harness counts
  itself, it appends to `LNK_MEASURES` (Link Harness,
  `modules/agents/harnesses/link/`, for its own).
- **[Files to the user's clouds](../../../modules/storage/storage/docs/services/storage/README.md#storage)**: `modules/storage/storage`.
  The cloud adapter uses the connected account's credentials to copy
  files directly to its storage.
- **[Connecting cloud accounts](../../../modules/cloud/cloud/docs/services/cloud/README.md#cloud-accounts)**, and machines in them, with [`lnk box`](../../../modules/compute/box/docs/services/boxes/README.md#boxes): `modules/cloud/cloud` and `modules/compute/box`.
  Neither knows which cloud, which is the adapter's job. A box is a
  machine running `lnk`, and what runs on it belongs to the module that
  runs it locally.
- **Support for one cloud** (AWS, Google Cloud, ...): a new module, an
  owner-local member answering the adapter commands
  `link_cloud_contract` declares. [Declare its kind and requirements in
  `module.toml`](../developing/modules.md#adding-a-cloud), and the commands it implements in `actions.toml`.

### Modules, commands and shared code

A new part, a new command, and what every module shares:

- **A new part of Link** (a new `lnk` group): [a new module](../developing/modules.md#adding-a-module), an
  owner-local member.
- **A new `lnk` command:** the module owning its group, declared in its
  `actions.toml` with its help, and [the shapes it takes in `types.toml`](../developing/modules/specs.md#add-a-commands-flags).
- **What [`lnk uninstall`](../modules/guide.md#uninstall) stops and deletes of a module:** that module,
  answering `uninstall` (`link_core::uninstall`). The core only asks.
- **Something every module needs:** `core/framework`, if it is small and
  truly shared.
- **How a secret is held** (never printed, zeroed when dropped):
  `packages/secretive` (`Secret`). A field holding a secret is a
  `Secret`, read with `expose()` only where it's used.
- **Documentation website:** `web/apps/docs`, within its web workspace.
  It keeps nothing and reads only the repository's docs.

#### A call Link makes over HTTP

A call to a service, such as the account API, a cloud API, or a chat
service, goes through `link_fetch::Client`
(`packages/fetch`, `link-fetch`), the only place Link's own code builds
an HTTP client: the request a `Serialize` type, the answer a
`Deserialize` one, its credential a `Secret`, and what differs [a named
rule of the client's](security.md#links-own-calls).
What an agent fetches (a page, an MCP server) and a model's provider
don't go through it, and each such client is a reviewed exception.

## Principles

What Link holds to in every part:

- **Every engine is swappable** behind a documented contract. A harness is
  any `lnk-<name>` answering the adapter commands, so closed code stays in
  its own repository, and Link never names it.
- **Link owns what stays the same when the harness changes.** That is
  settings, the files folder, the connections, switching, and which machine
  runs it. A harness that speaks a connection keeps speaking it, and for one
  that speaks none, Link carries the messages.
- **Work runs where it belongs.** A box routes, a model cloud thinks, a
  bucket remembers, a scheduler wakes. Nothing that belongs elsewhere runs
  on the box.
- **Link's harness runs no code.** Tools are MCP servers outside it,
  sandboxed, in any language, and shared by every harness. Keys never
  reach it, because the proxy adds them.
- **Turns hold no state between them.** Any process on any machine can
  take the next turn.
- **Conversations are append-only.** They are an event log that is never
  rewritten, and the prompt is sent the same way so the provider's cache
  holds. There is one layout on disk and in a bucket, in plain files a
  person can read, behind a storage seam. Tokens are always recorded, and
  cost only from a real rate.
- **Build the seams, not the implementations.** An interface has one
  implementation, and the others aren't built until something needs them.
- **Written for the densest deployment, run at the safest.** Code is
  written as if many agents share one process, with the agent always an
  argument, every path from one function, and a test that one agent can't
  reach another's. It runs one process per agent.
