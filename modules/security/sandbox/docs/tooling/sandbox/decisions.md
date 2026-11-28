# Decisions

Why the [sandbox](README.md) works the way it does. Why its specification is what
it is: [Specifications
Decisions](../../../../../../docs/config/specs/decisions.md#the-sandbox-specification).
Why its wall and its proxy work as they do is on their own pages:
[the wall's](../../../confine/docs/tooling/sandbox/wall/decisions.md)
and [the proxy's](../../../proxy/docs/tooling/sandbox/more/proxy/decisions.md).

## The module and what it closes

Why there is one sandbox, a module of its own that every harness asks
for each command it runs.

### Why is the sandbox a module of its own?

One sandbox serves everything: `lnk-sandbox`. Every harness runs in it
with a specification of its own (its folders, programs and network), and so
can any program or person. What one harness needs that another doesn't
is its specification, never a second sandbox. It is a module the parts share,
like [`lnk cloud`](../../../../../cloud/cloud/docs/services/cloud/README.md#platforms), not a part of its own.

### How does a harness use the sandbox?

It asks the [confinement client](../../../contract/client/docs/tooling/sandbox/more/contract/client.md#lnk-sandbox-wrap)
for each command it runs, the installer,
configure and the harness itself, and runs the command it gets back
rather than running through the sandbox module. The background service
then doesn't depend on the module at run time, and the caller keeps
what runs, where, and with which environment. The sandbox adds what no
specification chooses: Link's settings closed.

### How does the sandbox get installed?

With every harness: `harness` needs `sandbox`, and [`lnk mod add`](../../../../../../docs/more/modules/guide.md#add-or-remove-a-module)
follows needs through. A harness module installed before the sandbox
was its own adds it on first use.

## The wall, the way out and the bridges

Why the sandbox is three packages, and which module owns each bridge
behind its proxy.

### Why three packages: the wall, the proxy and the bridges?

The sandbox grew by adding each thing an agent needed to the one process
every agent ran, until it knew models, held secrets and spoke tools'
protocols.

#### Each a package

Now each kind of thing is a package that knows nothing of the one above
it: `link-sandbox-confine`, the wall (files, programs, the network
mode, a backend per OS); `link-sandbox-proxy`, the one way out (hosts,
endpoints, `[lan]`, the log, stop and pause, a table of routes and no
verbs); and the bridges behind it, each in the module whose purpose it
serves (models in `lnk-model`, secrets in `lnk-secret`, tools, `open`
and asking you in `lnk-sandbox`'s own).

#### Rebuilt whole

It was rebuilt whole rather than patched, so the sandbox Link comes with is
one design, not a layer over an earlier one.

### Why does an agent with no sandbox still get a proxy?

Its tools, memory, the scheduler, the wire's log, `lnk sandbox stop`
and counted usage don't depend on the wall, so they stay: the proxy
runs on the loopback behind its URL secret, and the agent is pointed
at it. Only the wall is gone, and the docs say the proxy is then
advice the program may ignore. An exit stays refused with no sandbox,
since nothing holds the program to it. (Not: "no sandbox" meaning
nothing of Link's between the agent and the network.)

### Why are `[lan]` and host mode two things?

`[lan]` names the machines on your network the proxy may reach, still
through it and still logged. "No filtering proxy at all" is
`[network] mode = "host"`, its own explicit choice, shown as loudly as
`--no-sandbox`. (Not: one switch meaning both.)

### Who counts a model's tokens?

The models module's route already reads each answer and knows each
provider's format, so it takes out each answer's token counts and hands
them to `lnk-measure`, which owns their type, the count, where it's kept
and [`lnk measure`](../../../../../tooling/measure/docs/tooling/measure/README.md#measure). Provider formats live in one module. (Not: measure
parsing every provider's format from a tap of the traffic.)

### Why are opening a page and asking you `lnk-sandbox`'s?

They're the sandboxed program's one way to reach you, so they're
bridges of the sandbox module's own, `bridges/open` and `bridges/asks`,
behind the proxy like any module's. A connection answering a question is
later one more place an answer comes from. (Not: a module of their
own.)

### Why are tools a bridge of `lnk-sandbox`'s?

Tools that run outside call `open` and asking you already, and every
agent's tools use the code: one more module would be one more to
install and secure. So they're `bridges/tools`, and `lnk sandbox tool
add` stays the command. (Not: a module of their own.)

### Why is the specification's schema made from the Rust type?

`Spec` is the one definition: its JSON Schema (`lnk spec show sandbox
--json`), its field flags, its generated reference page and the
sections `lnk sandbox permission list` lists are made from the type and its
doc comments, and a docs check fails when the page drifts. (Not: a schema
file with types generated from it.)

### Why is `developer_tools` a key of `[commands]`?

Xcode's and the Command Line Tools' programs are programs a sandbox
runs, so the choice to run them is `[commands]`', as
`[commands.macos] developer_tools = true`, read only on macOS. (Not: a
permission of its own beside the specification's sections.)

### Why does a run given only flags grant no program?

A specification's `[commands]` is closed when it names none, and a run's flags
are the same specification, so `lnk sandbox run --files.write . -- bash` starts
nothing until `--commands.allow` names `bash` or `'*'`. A file and its
flags mean the same thing, and the default is part of what every run
grants, so it is fixed rather than an opinion: changing it, either way,
is a breaking change. (Not: a flags-only run that allows every program.)

### How do a proxy and its agent share their socket folder?

When the proxy runs as another user than its agent's, they share the
folder by a group of their own, one per agent: the folder and its
sockets are `0770` and `0660` with that group, set-group-ID so a socket
made inside takes it, and the proxy accepts `in.sock` from the agent's
user alone. (Not: the proxy's user owning the folder and granting the
agent by ACL.)

## Tools, questions and the browser

How a tool that runs outside is reached, shared and approved, and how
Link asks you and opens a page.

### How is a tool at a URL reached?

Through a stdio server of Link's own, `lnk-sandbox mcp-remote`,
defined as the tool's command. Everything a tool gets, asking you for
each call, its sessions, the log, is the stdio tool's, so a remote
server gets it with no second path through the proxy. It runs outside,
so a server's token, sent as a header, is in the tool's definition in
Link's settings and never in a sandbox. A URL is `https://`, or
`http://` only on this machine, so a token never crosses a network in
the clear.

### Why can a tool run one server for every sandbox?

A tool's server runs once per session, so a dozen agents calling one
tool run a dozen servers, each with its own memory and start. A tool
with `--shared` runs one, under a hub that any proxy needing it starts
and that stops once idle (`shared_idle`), so a team's memory over its
folder runs once for the team, and nothing runs when no one calls it.
The hub speaks the same MCP over stdio's lines as any server, and
renumbers each session's requests, so a server written for one client
serves many without knowing.

#### Opt-in

Asking you stays each proxy's, before a call reaches the hub. It's
opt-in: a server that keeps a session's state between calls would share
it.

### How is a tool that runs outside named and approved?

`lnk sandbox tool add <name> -- <command>`, and every call asks you but
those `--add` names; `--ask none` asks for none. `--ask-if
<call>.<argument>` asks a call by one boolean argument, unless it's given
false, so a tool shaped as models are trained to call it, one call with a
flag, asks only for the case that needs it. A sandbox reaches
only the tools it names, as it does endpoints. A server's own word on a
call, MCP's `destructiveHint` or `readOnlyHint`, is shown but never
skips asking, since the server sets it itself.

### How does Link ask you?

A dialog on your screen, then the terminal with a code typed back, then
a question waiting for `lnk sandbox answer`; two minutes without one is
no. A tool's call with no dialog and no terminal here is refused at
once, naming what allows it: nobody would see it wait. Nothing in a
sandbox draws on your screen or writes Link's settings, so it can't fake
a dialog or answer one; on a terminal it types but can't read the code.
The queue makes a box askable at all. Silence is never yes.

### How does a sandbox open a page in your browser?

Through its proxy: `open` makes `$BROWSER` in the sandbox Link's own
program, which asks the proxy, outside, to open the page. There is no
LaunchServices and no desktop session inside. `open` takes the place
of `open-apps`, which let a program open anything on a Mac and so
amounted to no sandbox. A page opens in your browser with your cookies and logins, so
every one asks you and shows its whole URL, in plain ASCII so what you
read is what opens. No host is allowed in advance, since a URL on any
site can carry data out.

### How does a login's callback reach the sandbox?

A tool's login often waits on `localhost:<port>` for the browser's
answer. On macOS the sandbox listens on this machine's loopback, so the
answer arrives as it is. On Linux the sandbox's loopback is its own: the
proxy listens on the port the page's `redirect_uri` names, only for a
page you allowed, and passes the first request in. Then it closes, or
after five minutes, one port per page and never a range. A port already
taken is refused rather than shared, since the answer carries the
login's code.

## Sessions over SSH

How a person gets into a sandbox on a box, and what their session keeps.

### How does a person get into a sandbox over SSH?

Through OpenSSH, with a forced command: `restrict,pty,command="lnk-sandbox enter --spec <file> --folder <dir>"`. A shell, a command, `scp`
or `sftp` all run in the sandbox, and no forwarding gets around it. The
sandbox module keeps only that primitive; who gets a key and which specification
is theirs is the box module's (a box's users), since a person let in is
a user of a machine, not a part of its sandbox.

#### Not your own computer

(Not: people SSHing
into your own computer as you, each in a sandbox, confined by the
sandbox alone, where on a box each person is a Linux user of their own,
apart from the rest by the system too.)

### Why does a session keep its terminal?

A shell needs job control and Ctrl-C, so a session's sandbox skips
bubblewrap's `--new-session`: the terminal is their own SSH session,
which nothing else reads after. `lnk sandbox run` keeps a new session,
since the terminal it runs on is yours.

### Why is a session's specification checked each time?

The [SSH session entry](more/sessions.md) reads its specification as a run would, refusals and all, at
every session, so a folder that became a link since is refused.
`--check` reads it the same way and runs nothing, which is how `lnk box
user add` shows what a user's sandbox opens before adding them.

## Launch and policy boundaries

What of the policy builds apart from what enforces it.

### Policy types compile independently of their implementation

The authored policy, OS overrides, grant algebra, command policy,
network predicates and system-read roots are `contract/`, which links
no program, backend, proxy or module library, so credential holders
check those roots without a confinement backend. Its TOML parser and
path resolver are the shared `packages/spec`. The confinement backend and the filtering proxy are packages of their own, and only the sandbox module links them.
