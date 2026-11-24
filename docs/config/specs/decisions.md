# Decisions

Why Link's specifications are what they are: first what holds for every specification,
then each one's own.

## Every specification

What holds for every specification: one set of rules, one type for a
command's arguments, and tests that are files.

### Does each specification keep its own rules?

Every specification follows [the same rules](../../more/developing/modules/specs.md) for
names, paths and combining a file with command-line arguments. Each specification's
page describes its own fields, so learning another specification does not require
learning another configuration format.

### How do commands and specifications share their shape?

Each command has one typed invocation. Runtime arguments identify the
resource, choose output, or carry a program after `--`; reusable fields
belong to a registered specification kind. A kind's type supplies its TOML
reader, generated field flags, help, and reference. Flags and files use
the same validation and defaults. Flags override only the fields named.

### Does every command get `--spec`?

A command gets `--spec FILE` when it consumes a registered specification kind.
Simple commands still have typed arguments without making another
configuration format. A kind has one identity across modules that
compose it; a module's identity says who exports it. [`lnk spec list`](../../../modules/tooling/spec/docs/tooling/spec/README.md#list-the-kinds)
shows that ownership, and each kind's `spec show` inspects effective
configuration without creating a resource.

### Why is there one command entry?

Discovery, retired spellings, generated field flags, and runtime
arguments must agree before a command runs. The shared `link_spec`
entry applies those rules once and selects output from the parsed
invocation. Protocol payloads stay typed on their own boundary, and
forwarded program arguments retain their meaning after `--`.

### What does a field left out of a specification mean?

**The least that works: the narrowest access and the lowest cost,**
not whatever the command did before. Wide open is written down, as
`["*"]`, so reading a specification shows everything it opens.

### Is where a thing runs, or whose it is, in its specification?

**No: accounts, secrets and what Link picks per run** (which box, the
upstream) are the command's arguments, never a specification's fields, so a
specification handed to someone else holds nothing of yours. What it grants on
their machine (their tools, ports and network) is shown to them before
it runs.

### How are specifications tested?

**By TOML files, each a specification and what must come of it,** read by one
runner per group, so a new case is a file and not new test code.

### Why is the format separate from `version`?

`schema` identifies Link's file format, while `version` keeps the
meaning its owner gave it. Leaving `schema` out means current, so a
file written without format metadata keeps working. Published older
formats pass through their declared transformations before strict
validation, which keeps old shapes out of the current types.

### Why do reads leave the source unchanged?

The owner controls when a file changes. Reading transforms it in memory, and
an explicit rewrite preserves comments and ordering. Ambiguous changes
and changes that would widen access refuse with guidance instead of
guessing the owner's intent. The shared
[specification engine](../../../packages/spec/docs/more/libraries/types/specifications.md#versioned-documents)
owns these rules.

## The agent specification

Why an agent's opinions are a specification, how specifications meet, and who picks
where an agent's things are kept.

### Where are an agent's opinions?

In its specification, never in a harness or a bridge.

#### A decision, or an opinion

A decision is what anyone
with the whole evidence would choose (a platform's documented limit on
a bot's messages, a guard against a broken tool server), and lives in
the code; [a model's own figures are facts too](../../../modules/agents/harnesses/link/docs/agents/link/more/decisions.md#where-does-a-models-window-come-from), kept as data Link includes
and you correct.

#### What an opinion is

An opinion is a trade reasonable people make differently, most often
cost against what the agent can do, latency against memory, or
autonomy against control: how much of the window to pay for, how many
rounds a turn may take, a reaction while it answers. Each is a key of
the [agent specification](../../../modules/agents/agent/docs/config/specs/agent.md), and the test for which is which: would a
user with other constraints, looking at the same facts, choose
differently?

#### Link's opinions, as a specification

Link's own opinions are a specification Link includes, printed whole by [`lnk agent spec show`](../../../modules/agents/agent/docs/config/specs/agent.md#use-it), so someone with the opposite ones keeps every decision and
changes only a file. Specifications meet as sets: Link's, then a team's (what
its agents share), then the agent's own, a later key winning, and the
union must be whole. A missing key means Link's, not nothing, so a
specification of yours holds only what you'd change; `extends = "none"` is for
someone who wants none of Link's. Two teams that disagree refuse to
guess, and the agent's own specification settles it.

#### Thinking and the window

Thinking is one: on, a little, for a model that reasons, since the
better answer is usually worth a few hundred tokens, and off for one
that doesn't. The window is another: Link's specification sends at most 128,000
tokens of a conversation, so a long one never runs up a surprise bill
on a model that takes a million; someone who'd rather pay for more
memory sets `[window] max` higher.

#### Compaction and the rest

When a long conversation is
compacted, and whether the model saves to its notes first, are opinions
too: that it summarizes rather than forgets is a decision, but at three
quarters of the window or at half, and with an extra call to save
notes or without, are trades of cost against memory.

#### Each a key

How often a lease
is renewed and when it lapses, how often a streamed answer is shown
again, whether the scheduler asks first and in which time zone, and how
much of a model's window to keep for the answer or for counting roughly
are opinions too, each a key. Link's specification has no `[model] allow`, so any model may run: "the default
only" would refuse [`lnk agent start --model.default`](../../../modules/agents/agent/docs/agents/README.md#create-an-agent).

### Who picks where an agent's things are kept?

**The agent prefers; the machine decides.**

#### An opinion, and a fact

Where a thing is kept is an
opinion of the agent's (a conversation in SQLite to search it, or in a
file to read it) and a fact of the machine's (a machine that runs
agents only while they answer keeps nothing on its own disk). So the
agent's `[storage]` names a place for each collection, and the
machine's lists what it supports, its default first: the agent's opinion
holds where it can, and the machine's default where it can't.

#### It moves unchanged

The
agent never names where it runs, so a specification moves with it unchanged, and
what a machine overrode is printed, never written back into it.

### Why are places collections, and not formats?

**How a place keeps a thing is its driver's decision.** Asking for a
conversation as a log or compressed named a format, which held only on
a disk: one place a collection says what's wanted, and each driver
keeps it its own way (on `file`, a conversation is a file only added
to). Six collections, each one folder on `file`, give the agent's
tools one way into each.

### Why does a new agent write Link's specification out?

An agent's first start is where its defaults are chosen. So a new
agent's configuration saves Link's specification as concrete defaults in
`[behavior]`, beside setup in `~/Link/Safe/Agents/<agent>/agent.toml`.
Owners can read and edit their choices without resolving an `extends`
line, and turn off a feature a key at a time. Sandbox permissions stay separate
in `~/Link/Safe/Agents/<agent>/sandbox.toml`. Built-in tools start off, and adding one asks
before widening permissions. Deleting either file recreates defaults,
with missing permissions independently remaining closed.

## The box specification

Why a box is a file, what's in it, and how a cloud's table and the
matching work.

### Why is a box a file?

So the same box can be made again, and in another cloud: a box was
whatever [`lnk box create`](../../../modules/compute/box/docs/services/boxes/guide.md#create-a-box) had in its flags, and `t3.small` means nothing
to Google Cloud. A box specification says what machine you rent, as you'd
describe one at a store, and any box prints the specification that makes it
again.

### YAML, JSON or TOML?

TOML. Every file Link keeps is TOML, the `toml` crate is in already,
YAML's main Rust crate is unmaintained, and JSON holds no comments.

### What is in a box specification?

The machine you rent and the `lnk` on it, nothing that runs there:
agents, models, ports and setup scripts belong to the modules that run
them.

### Which is the source of truth, the file or the flags?

The typed definition. Files and generated field flags are equivalent
inputs to it; runtime arguments select the account and exact resource
name. A file is useful for repeating or sharing the machine configuration.

### How do the default and a cloud's own table combine?

`[machine.<cloud>]` replaces `[machine]`'s fields, each whole, so a
cloud's memory range never mixes with the default's. A cloud's `size`
wins over CPUs, memory, GPUs and arch, which a machine type has built
in, and giving both in one table is an error, since one would be
ignored.

#### Any cloud's kind

A cloud's table is named by the cloud's kind, any kind an adapter can
have, never from a list in the box module: a cloud whose adapter comes
from outside Link gets its own table, and its `--machine.<kind>.size` and `--machine.<kind>.region` flags,
with no change to [`lnk box`](../../../modules/compute/box/docs/services/boxes/README.md#boxes). A table for another cloud has
no say; a field the table doesn't have is refused as in `[machine]`.

### Is the account or the place in the file?

Not the account, which is whose box, not what box: a specification handed to
someone else means the same for them. The region is, in each cloud's
table, since a region names one cloud's place. `--name` stays the one
box's exact name for the same reason, and the specification's `name` is what its
boxes' names start with.

### `min`, `prefer` and `max`, or `min` and `max`?

`min` and `max`: the box gets the smallest that fits, so a preferred
size is a `min`, and `max` is the most you'll pay for when a cloud has
nothing at the minimum.

### Where does matching live?

In the box module, which knows no cloud: each adapter lists what it has
there (`sizes`), in the order it would pick them, and `lnk box` picks the
same way for every cloud. A specification with no CPUs, memory or GPUs gets the
adapter's default size, so an empty file makes the box Link made before
specifications, and an adapter without `sizes` still makes a pinned or default
size.

### Where does `lnk box spec show` read a box from?

Never from the box, where whatever runs could write the specification you make
the next boxes from. What it reads instead: [Security](../../../modules/compute/box/docs/services/boxes/lifecycle/on-a-box.md#a-box-specification).

### Why can't a specification install an older `lnk`?

A specification may be someone else's, and an older release may have a known
hole. So only a version you type yourself can be older than this
computer's [`lnk`](../../../modules/compute/box/docs/config/specs/box.md#lnk).

## The sandbox specification

Why a sandbox is a specification, where its type lives, and what a specification may
never open.

### Why is a sandbox a specification?

A sandbox is written down once, as a TOML file, then run, checked and
handed to someone else as it is. Each section is a grant and a section
left out is closed, so what a sandbox may do is exactly what the file
says. Every flag is a key of the same specification, so the command line can't
say what a file can't. A box's specification and a sandbox's follow the same
rules: TOML, strict, a missing key the least, `"*"` writing wide open
down.

### Where does a specification's type live?

In `link_sandbox_client`, beside `Confinement`, since more than the
sandbox module keeps one, and modules never link each other's code. What
a specification opens on a machine is one function there, `Confinement::of`; the
refusals that need the machine itself (a write that leads out, a program
it could replace) stay the sandbox module's, checked as it wraps:
another module has a specification checked by running [`lnk sandbox`](../../../modules/security/sandbox/docs/security/sandbox/README.md#sandbox), never by its
own copy of the rules.

### Is a harness's sandbox a specification?

Yes: what a harness runs with is two specifications joined, the agent's `~/Link/Safe/Agents/<agent>/sandbox.toml`, written by [`lnk agent permission add`](../../../modules/agents/agent/docs/security/sandbox/permissions/README.md#choose-what-your-agent-can-reach) and `deny`, and what the harness needs (`~/.lnk/safe/agents/harness-grants/<harness>.toml`),
and that is all there is. A harness comes with none of its own to add
yours to, and none with `[commands] allow = ["*"]`: it says each need
in `lnk-<harness> needs` as sections, with why, and Link asks
you for it. A harness whose sockets ignore the proxy needs
`[network] mode = "host"`, the network as it is, since `[lan]` goes
through the proxy too.

### Why is a missing `[commands]` closed in both inputs?

Changing the input source must never widen access. A file without
`[commands]` and flags without `--commands.allow` both grant no
programs. A grant names what it lets run: programs, folders of them, or
toolchains (`toolchains = ["shell"]`), each standing for the programs
it names. `allow = ["*"]`, alone, stands for any program, which an
agent's default sandbox file uses so builds and their outputs run. Any
other list keeps its executable restrictions.

### Why are a specification's refusals for files, not flags?

A file may be anyone's: one handed over could write `~/.ssh` or the
folder it's in, then widen its own next run. Flags are typed by you, or
by Link for an agent's own folder, which an agent's sandbox must reach.
So a file is checked whole, with the flags beside it, and shown before
its first run, while Link's settings stay closed to both.

### Why is a specification not run before shown first, and known by what it opens?

Specifications are written to be handed on, so one can come from anyone, and
what a file opens depends on where it is (relative folders), whose home
`~` is, and which `PATH` finds its programs. Its text, or a hash of it,
says nothing of that. So Link remembers the grants you allowed, every
folder resolved through links and every program as found, and asks
again when a specification opens anything else, a link moved under a folder it
names included.

### Why are folders of other programs' sockets refused in a specification?

`/tmp`, `/run`, `/var/run`, `/var/folders`, `$XDG_RUNTIME_DIR` and the
temp folder hold the sockets of a session's bus, Docker, an SSH agent
and a display, and on Linux a socket connects with a read grant alone.
What answers there runs what it's sent outside the sandbox, so a specification
can neither read nor write them, nor a folder holding one, `/` among
them.

### Why does a specification never run anything, nor turn the sandbox off?

One specification is one sandbox. Setup that needs other grants is a second specification,
run as its own sandbox before the first, so neither holds the other's
grants. And no key runs without a sandbox: to run without one, don't
run the sandbox.

### Why are a specification case's expectations `check`'s own flags?

A case in [`tests/specs/sandbox`](../../../tests/specs/sandbox/README.md) is a specification and what must come of it.
Each expectation (a host reached or refused, a program run or refused,
a folder's dot files written or not) is a hidden flag of `lnk sandbox
check`, a probe beside its own, so a case runs through the same sandbox
and probes a person does, and a new case is a file.

#### What counts as reached

A reach is the
proxy taking the call, its answer or still dialing after three seconds,
so a case needs no internet. A refused host is reached when the proxy
answers anything but its 403, but a name with no address here only
when it connects: the proxy's 502 for a name it can't resolve dialed
nothing, so a machine without DNS doesn't see a leak.
