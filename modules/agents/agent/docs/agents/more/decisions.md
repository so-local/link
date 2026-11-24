# Decisions

Why shared sandbox policy, proxy mediation and grants work this way.

## Sandbox

How a harness is sandboxed and kept to its proxy, how its bridges know
who calls, and whose its grants are.

### The sandbox

How the sandbox runs on each OS, where a harness's temporary files go,
and how to run something as your agent would.

#### How is a harness sandboxed?

On by default, and managed by Link: Seatbelt on macOS, bubblewrap on
Linux. The harness reaches its home, its agent's `home`, its own
localhost ports and a local model's, and system libraries.

##### Its grants

Everything else is a grant, a section of a [sandbox's
specification](../../../../../security/sandbox/docs/config/specs/sandbox.md):
the internet through its proxy and, on a Mac, the developer tools are on
by default; your home, your browser, this machine's other ports, your
network and the network as it is are off. `lnk agent permission add` and `remove`
change them for the agent, a harness's needs add to them while it runs,
and `lnk agent harness check` tests them. The sandbox is the sandbox
module's, so every harness runs in the same one, with its own folders
and grants.

#### How does Link handle Ubuntu's AppArmor rule for user namespaces?

With a profile for bubblewrap alone, added with your consent, rather
than turning the rule off with `sysctl`, which would open user
namespaces to every program. Never a silent fallback to no sandbox.

#### Where do a harness's temporary files go on a Mac?

In its home's `tmp`, its `TMPDIR`, with a `mktemp` of Link's first on
its `PATH`. macOS's `mktemp` without a template uses the per-user temp
folder whatever `TMPDIR` says, and the sandbox refuses that folder,
which every app shares: installers calling `mktemp -d` (uv's, in
Hermes's) fail. Link's makes the file in `TMPDIR` and hands a call with
a template to `/usr/bin/mktemp`. Opening the shared folder, even only
its `tmp.*` entries, would show the harness other programs' temporary
files. A program calling the C library directly, as `xcrun` does, still
meets the refusal.

#### Is there a package cache for installs?

No. A cache the sandbox can write to is one harness's install poisoning
another's.

#### How do you run something as your agent would?

`lnk agent harness run -- <command>`, under the harness's own policy,
beside the running agent. Without it, debugging an agent is guessing
from its log what the sandbox or its proxy did to a call. It builds the
same policy the harness gets rather than entering the running one, so it
works whether or not the agent runs, and it gets its own rules file and
proxy so it can't disturb the agent's.

### The proxy

How a harness is kept to its proxy, what gives it this machine's network
back, and how its install reaches the internet.

#### How is the harness kept to its proxy?

In a network namespace of its own (bubblewrap's `--unshare-net`),
crossed only by Link's relays over unix sockets, rather than firewall
rules: no root, no rules that go stale when the service restarts, and
anything ignoring the proxy has no network at all. On macOS, with no
namespaces, Seatbelt allows connecting only to the proxy's loopback
port. The proxy is the sandbox module's.

#### Is an agent behind its proxy on your own computer too?

Yes, whenever it has `[internet]` and the sandbox is on, not only on a
box or with an exit. The network as it is, with no proxy, opens every
localhost port and X11's sockets on Linux, and localhost through
`::ffff:127.0.0.1` and the Mac's own address on macOS, which the proxy
closes.

#### What gives an agent this machine's network back?

`[network] mode = "host"` (`lnk agent permission add --network.mode host`), off
by default: the network as it is, with the gaps it brings, which `lnk
agent harness check` shows. The proxy refuses this machine and its
networks by design, so a NAS or a model on another computer that `[lan]`
through the proxy doesn't reach needs it. An exit ignores it: its proxy
is its only way out.

#### Does `run` go out through the agent's route?

Yes: what `lnk agent show --permissions` lists is what every path of the agent
runs with, so a program `run` starts leaves as the harness does. With a
route (an exit), each call's proxy goes out through it, and `[network]
mode = "host"` and `[lan]` reach nothing past it, as for the harness.

#### Which ports does a harness reach through its proxy?

HTTPS's and HTTP's, and endpoints you name (`lnk agent permission add --internet.tcp
github.com:22`). A raw protocol reaches only what's named, and a name
reaches only its port, so nothing on the internet is reachable on a port
no one chose. With a list of hosts, both apply.

#### Where are an agent's hosts kept?

In its sandbox file, as `[internet] hosts`, and passed to its proxy as
its list, whichever harness runs it: the hosts are what you trust the
agent to reach, its model's and its connections', which a switch of
harness doesn't change. Link's file has `"*"`, any host, so a new agent
works from its first start.

#### How does a harness's install reach the internet?

Through a proxy of its own, direct from this machine, for each setup
step (the installer, `configure`, `link`): no step gets the network as
it is, so none reaches this machine or its network. The health check
gets no proxy and no network at all: only the harness's own ports on
this machine's loopback.

##### Which hosts

The installer and `configure`, which fetch code, reach only the hosts
the adapter lists (`lnk-<harness> internet-hosts`): what a harness installs is
the adapter's to know, where the running harness's hosts are yours.
`link` reaches any host, since a connection's servers are its own. The
exit isn't used: an install needs nothing from home.

#### Why does the health check reach only the harness's ports?

All it asks is whether the harness answers on them, and `lnk agent
show` asks it of every running agent. Behind a proxy it started a
proxy too, holding the agent's keys where the agent has them, for a
request that never leaves the machine. On the loopback alone it needs
none of that, and reaches less: no internet, no other port of this
machine, never a local model's.

##### When `status` runs it

`status` runs it only when nothing on the harness's ports answers a
plain request with success (a `2xx`): a port its proxy holds closes
without an answer, so one that answers with success is the harness, up,
known without a sandbox, and `status` costs a running agent about what
`status --json` does. Any other answer (a `503` from a harness without
its key) is the health check's to judge.

##### How long one run takes

One run of the health check, its sandbox's start included, has at most
15 seconds. A run still going then is ended, with everything it
started, and the harness counts as not answering. So `lnk agent show`
never waits on a sandbox that's slow to start on a busy machine. The
limit is `harness_waits.toml`'s, beside the code.

#### Why do the scheduler and `run` take the conversation from the harness?

Because nothing else knows it. Which conversation a call is made in, and
its hops, come in the call's `_meta` (`link.local/thread`,
`link.local/hops`), which the harness writes. The sandbox's tool proxy
forwards a call as it comes, and its secret is one per sandbox, not per
conversation, so it has nothing to check them against.

##### What that trusts

Link Harness writes `_meta` itself and runs nothing the model writes,
and `run`'s commands reach no tool. A harness that runs what the model
writes (a shell) could name another conversation, or reset its hops,
bounded then by the caps a day alone. A source the proxy could trust, a
token per message the bridge makes and the proxy checks, would change
the bridge, every adapter and the harness contract; [the trust is stated
instead](../chat/security.md#messages-from-others).

#### Where is a hosted model's key?

Added by the proxy, outside the sandbox (`lnk agent secret wire`). The
harness speaks plain HTTP to the proxy's local address and the proxy
speaks TLS to the provider.

##### Why not a CA of Link's

The alternative, a certificate authority of Link's trusted inside the
sandbox to read its TLS, would be a CA to protect, and this way only
providers the proxy knows get a key.

##### Whose harness holds it

It's on by default for a harness a test shows reaching its model through
the proxy, since a harness that ignores the address would fail with the
placeholder: Link Harness, OpenClaw, Hermes and DeepSeek Harness. A
harness from outside Link holds its key, since no test of Link's runs
it. `lnk agent secret` sets it either way for an agent.

### Bridges

How a bridge knows which agent calls, which key proves it, and why every
bridge and store driver runs as it does.

#### How does a bridge know which agent is calling?

By a proof made with the agent's own key, never the operating system's
word. Every way out of a sandbox but the proxy is a bridge (a
connection's, the way in, the store bridge), and each call to one is the
same three things: the agent's identity (its ID and owner, as Link
already defines them), the action, and an HMAC of a challenge the bridge
sends on that connection and that identity, keyed with the agent's key.

##### One type

One type in `link-sandbox-bridge` for every bridge, so none tells who's calling
its own way.

##### Why not the system's word

A Unix socket's permissions or its peer's process say which user or
program connected, not which agent, and differ from one system to the
next; a harness serving several agents, or one bridge serving many, is
one process to the system. A key proves only its own agent, wherever it
runs, so a harness, however it's steered, acts on a bridge only as
itself, and a signature gives the key away to no one. It's the
endpoint's proof turned the other way, under a label of its own, so an
endpoint, which signs any challenge, is never asked to make one.

#### Which key proves an agent's calls?

The one made at each start, which the harness already holds as its
endpoint's key: bound in the machine's keyring while the agent runs, and
unbound at its stop. A key that leaks then dies with the start.

##### Where an agent runs only while it answers

Where a machine runs an agent only while it answers, the key is made
each time something there is bound to run the agent instead: that
receives the agent's identity and its key as it's bound, from the
machine's own store of secrets, never from the agent's state, and
discards them when it stops, blank again for the next agent, which gets
a key of its own. What runs an agent there is then only ever one agent,
and only while bound.

#### Why does every bridge run on one server?

Because a bridge is a door out of the sandbox, and what keeps one
standing (a deadline before the proof, caps, a busy answer, an error
shape) is the same whatever it carries.

##### Written once

Each bridge once wrote its own socket loop, and each got some of it
wrong in its own way: a deadline on each read but not on the whole
proof, a cap counted before the proof, one call at a time, five shapes
of error. One server, in `link-sandbox-bridge`, does it once, and one test
checks every bridge against it, running; a bridge says only what it does
with a call and how many it holds. It's a library, not a module: no one
uses it on their own.

#### Why five seconds to prove, and unproven connections apart by user?

A caller with the key proves at once: the challenge is a line, the proof
a line. Five seconds over the whole exchange is far more than that
takes, and short enough that a caller dripping a byte at a time holds
nothing for long.

##### Places by user

Connections not yet proved hold places of their own, so they never take
one a proved call needs. Before its proof, the one thing a bridge knows
of a caller is the user at the other end, as the system names it: a
machine that packs many owners' agents runs each as a user of its own.
So each user gets a few places, and its newest takes the place of its
own oldest: a user flooding a bridge closes only its own connections,
never another user's proof, and room is kept for every user such a
machine runs.

#### Why does `send.sock` check the keyring too?

A connection's bridge is handed its agent's key as it starts, and could
check only that. But the keyring is where a key stops proving anything:
unbound at the agent's stop, replaced at its next start. Checking both
means a bridge that outlives its agent's stop takes nothing more proved
with the old key, as every other bridge already did.

#### Why does a store driver run outside the sandbox?

Because whatever a driver needs to reach its store, a credential or a
way to a database, would otherwise be in the harness's sandbox, where an
agent with a shell could read it and do anything the credential lets it.
Behind the store bridge, the harness asks for the store's operations
alone (append if the number is free, read, the lease), as itself, and
only for its own conversations; the bridge runs the driver outside, and
the harness neither knows nor cares which it is.

##### Another driver's conversation

A conversation kept by another driver than the specification's now is read as
before, the driver run where its caller runs, until it's moved.

#### Why does the store bridge keep a driver running?

Because a process and a fresh connection to the store for every call are
most of a short turn's store calls: a turn makes a dozen or so, and each
paid a process's start and a connection's handshake on top of its one
request, about twice the time at 30 ms to a database.

##### `serve`

A driver whose contract says `serve` answers call after call on one
process and the connection it holds, so a call costs its one request. It
stays one call at a time, in order, stopped past 30 seconds as before,
and a driver that doesn't say `serve` is run once a call as it always
was, so no driver had to change.

##### Why not a library

The alternative, a library each driver links into the bridge, would put
every driver's code and credentials in one process and tie drivers to
Rust.

### Grants

Whose each grant is, why the agent's file is a sandbox specification, and who may
turn the sandbox off.

#### Whose are the grants?

Two layers, each in its own file. What you grant an agent is the agent's
(`~/Link/Safe/Agents/<agent>/sandbox.toml`, `lnk agent permission add`): it follows
the agent to any harness, so a switch never asks for it again, and two
agents on one harness can differ.

##### The harness's

What a harness needs beyond that is the harness's
(`~/.lnk/safe/agents/harness-grants/<harness>.toml`): what it is decides it (OpenClaw's
sockets ignore the proxy whichever agent runs it), so it's asked when an
agent switches to it, held only while it runs, never given to another
harness, and the same for every agent here that runs it.

##### Only what it asked for

A harness's layer holds only what it asked for: `lnk agent
harness-permission add` and `remove` change a need it declares, and refuse anything else,
which is `lnk agent permission add`'s to grant the agent; `reset` forgets the
answers, so its next start asks again. Allow and deny mirror each other,
so taking a need back is never a one-way door. A harness that needs
nothing runs with the agent's alone.

#### Why is the agent's file a sandbox specification?

So there is one way to say what a sandbox may do. The agent's file, a
harness's grants and its needs are a [sandbox
specification](../../../../../security/sandbox/docs/config/specs/sandbox.md)'s
sections, and `lnk agent permission add` and `remove` take the flags `lnk sandbox
run` takes, so what you learn for one you know for the others.

##### In the agent's terms

The file is in the agent's own terms: `[files]` names the folders it
sees (`work`, `instructions`, `.` for all of Home), and a section a
harness's sandbox can't enforce is refused, naming the line, never a
grant that silently doesn't hold. Its `[files]` and `[commands]` hold
where Link enforces them: `[files]` in a harness that keeps to the
layout and in Link's tools, `[commands]` in each of `run`'s sandboxes.

#### What does a member reach of its team's folder?

Only what its own sandbox file grants it, `[teams.<team>]`: the folders
of the shared folder it writes, and those it only reads, or all of it.
Two members' sets may overlap or not, so a checker can read the sources
no one else does.

##### Hidden but for what's granted

Every sandbox hides `~/Link/Teams`, and each team's folder kept
elsewhere, but for what's granted inside, as it hides other agents'
folders, whichever harness runs the member and whatever `--files.read '~'`
allows.

##### The agent's grant

The grant is the agent's, not the team's, so it follows the agent like
its other grants; being on a team is still what lets it reach that
team's folder. `lnk agent team add` grants the whole folder unless told
otherwise. `lnk agent harness check` reads each team folder the member
is granted (it must) and every other team's (it must not).

#### Why is no sandbox the agent's setting, not a harness's grant?

The agent owns one `sandbox` setting. `lnk agent start --no-sandbox`
sets it to false, `--sandbox` turns it back on, and the setting persists
across starts and appears in status. A harness reports whether it can
run confined through its `needs` answer. A start that requires an
explicit choice refuses until the person supplies `--no-sandbox`. The
sandbox is enabled by default.

#### Who can turn the sandbox off?

You, or your script, from outside any sandbox, by name; never an agent.
Running without one is reasonable on a machine you know, or in a test,
so a script may, with `--no-sandbox`, the only way. `--yes` answers what
a harness asks and never that, so a script that starts or restores an
agent doesn't turn its sandbox off by running a harness that asks: no
sandbox reads and writes all you can.

##### Never an agent

An agent can't turn its own sandbox off, or another's: what decides it
(its settings, where `--no-sandbox` is kept, its specification, its sandbox file,
its harness's grants, the machine's settings and the services' units) is
all outside the folders its sandbox may write, and tests try each route
at each from inside one.

##### `unsandboxed = false`

A deployment that must never run an agent unsandboxed (a cloud starting
harnesses for others) sets `unsandboxed = false` in
`~/.lnk/safe/agents/policy.toml`: then every way there is refused, and a harness
that can't run in a sandbox doesn't run.

##### A service already running

A service already running a harness unsandboxed reads it too: it runs
the harness through `lnk agent unsandboxed`, which checks the file at
every restart and boot, and removes the service rather than run it,
since systemd and launchd restart a service without asking `lnk`.

##### The file on every machine

The policy file may hold the switch alone, written before `lnk` first runs.
Installation identity is kept separately. It's read only when you (or root) alone could
have written it. A computer and a box use the same rule: sandboxed is
the default, `--no-sandbox` is the explicit choice, and a trusted
`unsandboxed = false` is the machine owner's refusal.

#### Why does removing a harness keep its grants for another agent?

`lnk agent harness delete --harness <harness>` removes one agent's home of it. Its
grants go with it, so adding it again asks again, unless another agent
here runs it: deleting them would change what that agent runs with at
its next start.

#### What happens to a sandbox file that names permissions?

It's read once, through one table, and written back as a specification's
sections, the same table for an agent's file, a harness's grants and a
move's manifest.

##### Never widened

What a section says as broadly is kept, and what it can't say as
narrowly is left out and said once, never widened: a `localhost` without
`lan` reached every port behind its proxy, so it's left out, and each
port it needs is named with `--localhost.ports`; `lan` without `network` opened
nothing, and is left out too. One thing is gained, the one exception:
ports named without `network`, which only macOS reached, reach on Linux
from then on, relayed into its sandbox as on macOS, and that's said as
it's read.

### What a harness asks for

Whether a harness says what it needs and risks, when Link asks, and what
the ask covers.

#### Does `[commands]` bind the harness's own programs?

Yes, as it binds `run`: what `lnk agent show --permissions` lists is what every
path of the agent runs with, so a harness names the programs it runs,
past the program Link starts and its `#!` interpreter. One that runs
what it installed in its home asks for that folder and the toolchains
its tools use, each naming its programs, never for any program.

#### Does a harness say what it needs?

Yes, `lnk-<harness> needs`, with why, in the harness's words; Link
doesn't change a harness to fit its sandbox. It reads the settings,
since what it needs can depend on them (a model provider, a connection),
without keys or tokens, since it runs outside the sandbox. An unknown
name is refused, so a typo never runs a harness with less than it needs,
or more than it said.

##### A specification

A need is a sandbox's specification, as [`lnk sandbox run --spec`](../../../../../security/sandbox/docs/security/sandbox/guide.md#sandbox-guide) takes it, with
no name, version or relative path, so it's shown and taken back as the
flags `lnk agent permission add` takes.

##### Version 2

The contract's version 2 made needs specifications: an adapter of version 1,
whose `needs` names permissions, is refused, saying it's older than this
Link and to update it, rather than read as something it didn't say. An
adapter that doesn't answer it (an error exit, or no answer) needs
nothing more, since an outside adapter releases on its own schedule.

#### When does Link ask?

Before the harness runs: `use`, its first start, a start after its needs
grew, and a move, in the terminal running it, before the agent starts
where it goes: the machine it goes to answers what it needs there, and a
refusal removes the copy put there, so the agent starts where it was and
never runs twice.

##### Kept with its answers

The answer is kept with the needs it answered, so it asks only for
what's new, and never again for what you allowed or took back: a deny is
an answer too, until `lnk agent harness reset` forgets them. What the
agent has of its own is never asked. Without a terminal, it refuses and
names `--yes`, rather than run a harness without what it needs, or with
what nobody allowed. The question names the agents here that run the
harness, since the answer holds for each.

#### Why does every switch that widens what an agent may do ask?

Because a harness's needs are its own, so switching an agent to another
harness can give it more than it had, and the sandbox is only worth what
the user knows of it. `lnk agent harness set` lists each need the new harness
holds beyond the old one's and the agent's own, with what it opens,
beside what the new harness asks for now, and asks once, in the warning
color; no keeps the agent where it was.

##### At every switch

It asks at every such switch, not once per harness: the agent and the
harness it leaves are what change, and a grant already answered for a
harness is still new to an agent coming from one without it. Less is
said in a line and asks nothing.

#### How does a harness say what running it risks?

By a risk of its own in `needs`: a name, and why. Some of what a harness
does opens a door no specification names, such as a port of its own that asks for
no credential, which every user of a Mac reaches on its loopback.

##### Named by the adapter

Link can't know each harness's ports, and the fix is the harness's to
make, so the adapter names the risk and Link asks for it as it asks for
a specification, then hands the adapter those allowed (`allowed`) as it's
configured.

##### One it can run without

A risk the harness can run without (OpenClaw's managed browser) says
what it does without it, and only a yes in a terminal turns it on: off
is safe, so `--yes` and no terminal leave it off rather than fail. One
it can't run without is asked as a specification is, and taken back, the adapter
refuses to start. Its name is refused where `permission add` or `remove` takes the
name for something else, and the field is new without a new version: an
older Link refuses an answer with it rather than running without asking.

#### How does an adapter know where its ports are?

Link tells it whether its harness runs behind its proxy (`proxied`), not
which grants it has. On Linux behind its proxy the harness has a network
of its own, and its ports other than those the proxy serves stay there;
without it they're on the machine's loopback.

##### One fact

No sandbox, `[network] mode = "host"`, and anything later that takes a
harness off its proxy all come to that one fact, so an adapter that
names a risk by it needs no change when a new way off the proxy comes.
The adapter adds what it knows: that macOS has no network of the
sandbox's own, and a `--network.mode host` it asks for itself, which
`needs` hears before it's granted.

#### Can a harness ask for less than the defaults?

No: a harness says only what it needs beyond them. A harness with no use
for the internet runs with it unless you deny it.

#### Does the ask cover hosts?

No: it covers what a harness needs beyond Link's defaults, the grants
that widen the sandbox past its proxy, and risks of its own. The agent's
hosts stay as they are, any until you list them, and the hosts its
installer reaches stay its adapter's `internet-hosts`.

### Why are only this machine's permissions listed?

`--commands.developer_tools` opens Xcode's tools, which only a Mac has, and the
network and a port open different things under Seatbelt and bubblewrap.
A list that names another OS's grant, or describes one as it works
there, makes a person reason about a machine they aren't on. So `lnk
agent show --permissions`, help, and what a grant says show this OS's own, each
as it works here. Another OS's table (`[commands.macos]`) is still read
and kept, never refused: an agent's grants move with it between a Mac
and a box, and a script works on both.

## Decisions of other parts

Each part keeps the decisions about its own code on its own page:

- [Portable context](../../../../harnesses/portability/contract/docs/agents/harnesses/more/protocol/decisions.md#why-is-portable-context-independent-of-connection-transport),
  in the harness contract's
- Link Memory: [Decisions](../../../../tools/memory-tool/docs/agents/tools/memory/more/decisions.md), what it writes, indexes and searches
- Link's tools: [Decisions](../../../../tools/docs/agents/tools/more/decisions.md), where they live, a team's tool and delegation
- Web: [Decisions](../../../../tools/web-tool/docs/agents/tools/web/more/decisions.md), [Run](../../../../tools/run-tool/docs/agents/tools/run/more/decisions.md),
  [File](../../../../tools/file-tool/docs/agents/tools/file/more/decisions.md) and [Code](../../../../tools/code-tool/docs/agents/tools/code/more/decisions.md)
- Telegram: [Decisions](../../../../../../interop/agents/connections/telegram/docs/agents/connections/telegram/decisions.md),
  [Slack](../../../../../../interop/agents/connections/slack/docs/agents/connections/slack/decisions.md),
  [Discord](../../../../../../interop/agents/connections/discord/docs/agents/connections/discord/decisions.md) and
  [Signal](../../../../../../interop/agents/connections/signal/docs/agents/connections/signal/decisions.md), each connection's bot or app
