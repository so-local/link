# Interface

> Interface, One Contract for Every Harness


A harness program, `lnk-<name>`, answers a set of commands and speaks
typed conversations. This page lists both.

## Harness programs (`lnk-<name>`)

`lnk-harness` and every harness adapter from outside Local Link answer
these commands. The contract's source is
[`harness.rs`](https://github.com/so-local/link/blob/main/modules/agents/harnesses/portability/contract/src/harness.rs),
and the authenticated storage and context protocol is in
[Portable context](continuity.md). What every harness shares apart from
how Link runs it is in [Harness Base](../../../../../base/docs/agents/harnesses/more/vocabulary/base.md),
the words its messages and skills are written in are in [Harness
Wire](../../../../../wire/docs/agents/harnesses/more/vocabulary/wire.md), and the
[Conformance Kit](../../../../../../conformance/docs/agents/harnesses/more/protocol/conformance.md)
checks a harness program answers all of it.

### Command Declarations

The protocol's commands are declared once, in the contract crate's
[`actions.toml`](../../../../../actions.toml), with what each answers
in its [`types.toml`](../../../../../types.toml). `state` on its own is
the `state` group's default, beside `state export`, `state schedules`
and `state clear-schedules`. Each harness program lists the commands it answers
under `[implements]` in its own `actions.toml`, and its command line is
generated from that, so every program reads a command the same way.

### Commands a Program Extends

A harness program that takes more than the protocol's shape declares that command
as its own: Link Harness's `serve`, with `--agents` and `--control`, and
Hermes's `state clear-schedules`, with the context its exported
schedules name. Settings and portable state remain typed requests on
standard input. Extra arguments are refused before those requests are
read.

### Versions

The contract has a version, the harness protocol's, which
`harness/protocol.toml` declares (`link_harness_contract::harness::PROTOCOL`,
now 2) and a change to it advances. An agent refuses to start a harness
whose adapter states an unsupported version, including zero, by name, naming
[`lnk upgrade`](../../../../../../../../../../docs/more/modules/guide.md#upgrade).

### Over the Agent Client Protocol

The [Agent Protocol](../agent-protocol.md) brings the [Agent Client Protocol
(ACP)](acp.md) and Link's Agent State Protocol (ASP) together. ACP provides
sessions, prompts, streaming, cancellation, and capability discovery.
ASP adds optional portable-state exchange and turn steering. This
adapter interface supplies installation, launch, and native-state
operations around those protocols.

## Where each command runs

`install`, `configure`, `command` and `health` run inside the sandbox,
unless it's off; `traces`, `written`, `clean`, `connections`, `state`, `internet-hosts`, `needs`,
`endpoint` and `contract` run outside it. Informational commands print
replies; `clean` edits only the selected migration copy.

### Their environment

The four inside the sandbox run with `HOME` set to the harness's home,
`TMPDIR` its `tmp/`, `LNK_FILES` the files folder, the agent's Home,
and `LNK_AGENT_FOLDER` the agent's own folder (`agent/`, its collections
kept at `file`). `LNK_AGENT` is the agent's name, `LNK_AGENT_ID` its ID
and `LNK_OWNER` its owner (`acct-<id>`, or `<user>@<machine>` before you
sign in).

### The agent's ports

`LNK_PORT` is the agent's base port, set for every agent, `main` too,
since another user's agent may hold the adapter's default. Its ports go
from there, within as many as its
contract's `ports` and one more, or 200 when it doesn't say, and
`Launch.ports` outside that are refused.

### Its own measures

`LNK_MEASURES` is a file in its home where the harness may append its
own measures, a line of OTLP JSON at a time, holding the file's lock
(`flock`, `LOCK_EX`) as it writes (`link_measure_contract::client::append`):
what it counts itself, a turn's tokens, cost, calls and times. The rest
is dropped as [`lnk measure`](../../../../../../../../../tooling/measure/docs/tooling/measure/README.md#measure) reads it, and the file emptied, holding the
lock, once every line is read.

### Asked once

`contract`, `connections`, `internet-hosts`, `endpoint` (for each base port) and
`needs` (for each input) are asked once a process while the adapter's
program stays the same file, and again once it's replaced or rewritten.

## Set it up and run it

The commands `lnk-agent` asks to install the harness, configure it, run
it and check it answers, each assumed as it says when an adapter
doesn't answer it.

### `lnk-<name> contract`

What the adapter speaks and its harness takes, asked once a process:
the contract's version and each of its parts, on [Harness Contract](contract.md).

### `lnk-<name> install`

Installs the harness with its own installer, or does nothing, and asks
nothing. It says so first when it installs, and shows progress: its
installer's own output, and every 30 seconds beside it a line
saying it's still installing and for how long (the contract's
`progress::Heartbeat`, its period in `progress.toml`), so neither a
person nor a program watching its output takes a long download for a hang.

### `lnk-<name> configure`

Writes the harness's own settings from the agent's settings:
[Settings](settings.md). [`lnk-<name> needs`](settings.md#lnk-name-needs), beside it, says what the
harness needs beyond Link's defaults.

### `lnk-<name> command`

Prints `Launch` as JSON, how to run it in the foreground: `{"argv",
"env", "write", "read", "ports", "serve"?, "services", "wait_secs"?}`.
`lnk-agent` refuses one that would widen the sandbox.

#### `serve`

Of `ports`, those Link reaches from outside the sandbox (its gateway or
endpoint, its health check), the only ones served on this machine's
loopback behind its proxy. Left out, all of `ports`.

#### `wait_secs`

How long the harness may take to answer `health` after it starts, at
most 600. Left out, 60 seconds (`Launch::wait`).

### `lnk-<name> health`

Exits 0 when the running harness answers, reaching it on its ports on
this machine's loopback, the only network it has in the sandbox. Read
by [`lnk agent start`](../../../../../../../../agent/docs/agents/README.md#start-your-agent), and by [`lnk agent show`](../../../../../../../../agent/docs/agents/models/agent/README.md#set-its-model-and-how-hard-it-thinks) when nothing on its
saved ports answers a request with success, a `2xx`. What does is the
harness, up, its proxy closing without an answer; any other answer,
such as a `503`, is the health check's to judge.

### `lnk-<name> internet-hosts`

A JSON list of the hosts its `install` and `configure` reach, as `lnk
agent permission add --internet.hosts` takes them (`["registry.npmjs.org",
"*.githubusercontent.com"]`). With the sandbox and `[internet]`, every
step runs behind a proxy of its own, direct, and those two reach only
these hosts; an adapter without the command lets them reach any host on
the internet.

#### What the other steps reach

`link` reaches what the agent may while it runs (`lnk agent permission add
--internet.hosts`, `--internet.tcp`; `"*"`: any host), and `health` only the
harness's ports on this machine's loopback.

## What it says of its harness

The commands that say what the harness speaks, where it's reached, what
it keeps and where it writes, each read by the [`lnk agent`](../../../../../../../../agent/docs/agents/README.md#agents) command
named.

### `lnk-<name> connections`

A JSON list of the connections its harness speaks, by their modules'
names (`["telegram", "slack", "discord", "signal"]`); an adapter
without it speaks Telegram. Read by [`lnk agent connect`](../../../../../../../../agent/docs/agents/connections/connect/README.md#choose-an-app), which refuses
one the harness lacks, [`lnk agent harness set`](../../../../../../../../agent/docs/agents/harnesses/use/README.md#switch-in-one-command) and `lnk agent show`.

### `lnk-<name> endpoint`

For a harness that speaks no connection itself, where Link posts its
messages: [Harness Endpoint](endpoint.md).

### Conversations

`lnk-<name> instructions [--json]` prints the system prompt its harness
sends its model now, `lnk-<name> conversation <thread>` a conversation
as a person reads it, and `lnk-<name> conversations move <way>` moves
the agent's conversations to another way of keeping them:
[Conversations](conversations.md).

### `lnk-<name> state`

Prints `link_harness_contract::harness::State` as JSON: `{"include",
"exclude"}`, paths `~/...` in its home that hold the agent, and those
under them that are its install (programs, runtimes, logs, caches,
`.env`). Read by `lnk agent state-send`.

### `lnk-<name> link <connection> [--again]`

For a linked connection (`describe`'s `linked`: Signal), whose account
the harness links itself: links it in the home, its QR code on the
terminal, once configured. It exits 0 at once when it's linked
(`--again`: links it anew), and 10, [`NOT_LINKED`](../../../../../base/docs/agents/harnesses/more/vocabulary/base.md#a-link-not-made),
when it isn't and there's no terminal.
Read by `lnk agent connect` (`--again`), `lnk agent start` and `use`,
before starting the harness.

### `lnk-<name> traces`

A JSON list of each place the harness writes when it isn't in its home,
and what kind of thing is there: `{"kind": "home", "path":
"~/.<name>"}`, the path `~/...` or absolute, `{uid}` for the user's id.
Read by [`lnk agent harness show`](../../../../../../../../agent/docs/agents/harnesses/use/harness/README.md#inspect-the-installation), which labels each by its kind, and
[`lnk agent harness delete --harness <harness>`](../../../../../../../../agent/docs/agents/harnesses/use/harness/README.md#remove-the-harness).

#### Its kinds

A kind is `home`, `config`, `secrets` (a `.env`-style file of keys and
tokens), `login` (one of its own that can't move), `service`,
`program`, `package` (what its installer put beside it), `cache`, `log`,
`lock`, `browser`, `agent` or `profile`; any other entry is refused.

#### An install's agents

An `agent` path is one of several agents sharing the home: `{agent}` is
its name, `{-agent}` a dash and its name, and for the install's own
agent `main` and nothing (`~/.openclaw/workspace{-agent}`). A `profile`
path is another agent's whole home (`~/.hermes/profiles/{agent}`).
[`lnk agent migrate`](../../../../../../../../agent/docs/agents/README.md#migrate-to-local-link) makes each an agent of Link's.

### `lnk-<name> written <home>`

A JSON object naming the version that wrote the install whose home is
`<home>`, and why Link can't run that state when it can't:
`{"version": "2026.9.6", "refused": null}`. Read by `lnk agent migrate`,
which refuses a state with `refused` set before copying anything.

### `lnk-<name> clean <home>`

Takes the secrets on its stdin, a JSON list of their values, out of the
files of its own in the home `<home>` keeps them in, other than its
`secrets` traces, which Link edits itself. An adapter whose `contract`
says `forget` answers it. `lnk agent migrate` runs it on its copy of an
install for each secret it takes, so the copy keeps none of them
before the agent's first start.

### `lnk-<name> uninstall --parts --json`

As [every module's](../../../../../../../../../../core/framework/docs/more/modules/protocol.md#uninstalling-lnk-module-uninstall),
run outside any home: the credentials its harness keeps in each agent's
home (`link_harness_contract::harness::homes`:
`~/.lnk/data/agents/<agent>/harnesses/<name>`), such as a token or a linked
device it made itself, as part `agents`'s `credentials`, with no paths:
the folders are the harness module's. Read by [`lnk uninstall --parts`](../../../../../../../../../../docs/more/modules/guide.md#uninstall) and [`lnk secret list`](../../../../../../../../../security/secret/contract/docs/config/secrets/more/contract.md#listed-for-lnk-secret-list).

#### Without it

An adapter without `uninstall` holds nothing `lnk secret list` shows, so
the list names no credential in its agents' homes.

## Adapters from outside Link

Any `lnk-<name>` answering the commands above is a harness: `lnk agent
harness set <name>` runs it like Link Harness. A harness Link doesn't write
lives in its own repository, and releases and signs itself.

### Releasing

A GitHub release holds `lnk-<name>-<target>.tar.gz` for each target it
runs on (`aarch64-apple-darwin`, `x86_64-apple-darwin`,
`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`), each with
`lnk-<name>` alone. `lnk` on a machine whose target it lacks refuses it,
and so does a box of that processor. `lnk-<name> --version` prints
`lnk-<name> <version>`.

#### Its signature

The release holds a `SHA256SUMS` of its archives, signed with the
repository's own Ed25519 key (`ssh-keygen -Y sign -n link-release -f
<key> SHA256SUMS`, giving `SHA256SUMS.sig`), and the key's public half
as `release-key.pub`.

### Installing

[`lnk mod add --url https://github.com/<owner>/<repo>`](../../../../../../../../../../docs/more/modules/guide.md#add-or-remove-a-module) takes the latest
release, shows the key and asks, then [pins both](../../../../../../../../../../docs/more/modules/guide.md#add-a-module-from-outside-link).
It asks the program only `--link-describe`, installing any module that
describes itself by its name but a standalone program.

### Updates, lists and boxes

[`lnk upgrade`](../../../../../../../../../../docs/more/modules/guide.md#upgrade) takes each one's latest release, signed by the pinned
key, when it's newer. `lnk agent harness set` lists it with where it came from, as [`lnk mod list --json`](../../../../../../../../../../docs/more/modules/guide.md#get-help) does, and [`lnk agent move`](../../../../../../../../agent/docs/agents/move/README.md#move-your-agent-to-the-cloud) adds it on the box from
the same repository, with the same key.

### Without a release

A `lnk-<name>` you put on the `PATH` yourself works too, by name,
unsigned, and yours to update and put on a box.

### The smallest one

The smallest adapter answers `contract`, `command` (its `Launch`),
`health`, `needs` and `endpoint`, and serves ACP at that address: Link carries Telegram for it, and the sandbox, the model's key
on the wire, moving and switching work as for any harness.

## Shared Vocabulary

[Harness Vocabulary](../vocabulary/README.md) connects the protocol-free
base, thread, skill, and in-process runtime types used at these boundaries.

## Implementation References

[Harness Hub](../../../../../../hub/docs/agents/harnesses/more/protocol/hub.md) explains native process and session ownership. [Harness Prompts](../../../../../wire/docs/agents/harnesses/more/vocabulary/prompts.md) defines the in-process runtime boundary, [Runtime Comparisons](../../../../../../conformance/docs/agents/harnesses/more/protocol/runtime.md) describes controlled fixtures, and [Decisions](decisions.md) explains the contract’s state-transfer choices.

<!-- docs-tree:start -->

## Docs

### Pages

- [Settings](settings.md): What a harness program is told of the agent it runs, and what it says its harness needs: `lnk-<name> configure` writes the harness's own settings from the agent's configuration, and `lnk-<name> needs` reads them to say what the harness…
- [Harness Endpoint](endpoint.md): How Link carries the messages of a harness that speaks no connection itself: its adapter names its endpoint, where it speaks the Agent Client Protocol (ACP), and each message from its connections is a prompt there
- [Conversations](conversations.md): The commands that show what a harness tells its model and the conversations it keeps, and the typed Link conversations that a harness may answer as well
- [Portable Context](continuity.md): An adapter can capture readable conversation records and read shared memory through Link's authenticated storage bridge, whichever carries its connections
- [Agent Client Protocol](acp.md): The Agent Client Protocol (ACP) is an externally maintained open standard for connecting clients to agents, letting a client prompt an agent and receive its responses and tool activity through a common interface

### More

- [Harness Contract](contract.md): What a harness program says of itself when asked `lnk-<name> contract`: the version of this contract it speaks, and each part of the contract its harness takes
- [Decisions](decisions.md): Why a harness program meets Link as it does, where the interface leaves a choice

<!-- docs-tree:end -->
