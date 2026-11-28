# Filtering Proxy

> Proxy Filters the Traffic You Allow

The package `link-sandbox-proxy` is the sandbox's one way out, a
filtering proxy that Local Link runs outside the sandbox, with a table of
routes that names none of them. Every sandboxed agent with `[internet]`
runs behind the proxy unless it has `[network] mode = "host"`. Any
sandbox can have the proxy instead of the network with
`lnk sandbox run --internet.hosts` (or a specification's `[internet]`), and
everything below applies to it except a SOCKS upstream.

## Where Proxy Traffic Leaves

On this computer it leaves from here, upstream `direct`. On a box it's
always behind the proxy, whatever its `[network]`: [`lnk agent move`](../../../../../../../../agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud) sets
its route, and a new box's setup makes it the box's default, which every
agent made there starts with and one already there with no route takes.
From a box it leaves from the box itself (upstream `direct`), or only
through the SOCKS proxy its route names.

### SOCKS Upstream on a Unix Socket

A SOCKS upstream on a Unix socket must be a socket owned and served by this user or root. Symlinks and
other file types are refused. Linux pins its opened inode across the
connection; every supported OS checks the connected peer's UID, so a
path swapped to another user's server is refused before the SOCKS
greeting is sent.

### HTTP Proxy Upstream

An upstream that's an HTTP proxy on this machine's loopback
(`http://127.0.0.1:<port>`) is handed each internet host's name,
unresolved, as `CONNECT <name>:<port>`: it resolves the name, and must
refuse what this proxy would (this machine, its network, a metadata
service), since nothing here checks the address it reaches.

#### What's still checked here

The hosts the sandbox may reach are still checked here, by name, before
it's asked, an address the sandbox names is checked here as any, and
`[lan] *` reaches no other name. A machine whose resolver must not be
asked a name a sandbox chose uses it: its DNS then answers only the
endpoints' own names, an allowed name has no records, and only a name
`[lan]` lists by itself, or one of no internet's, is dialed here.

## Only Way Out of the Sandbox

Anything that doesn't use a proxy has no network at all, and [no DNS but
its proxy's](connect.md) where it names endpoints. The proxy carries TCP, not UDP.

### On Linux

bubblewrap gives the harness a network namespace (`--unshare-net`): its
own loopback, nothing else. Its only way out is a relay inside the
sandbox, `127.0.0.1:40781` and its `HTTPS_PROXY`, to a unix socket
where Link's proxy, outside the sandbox, takes HTTP `CONNECT` and plain
`http://` requests.

### On macOS

With no network namespaces, the proxy listens on a free port of the
machine's loopback. The sandbox's Seatbelt rules allow connecting to
that port and the specification's own, nothing else: not `::ffff:127.0.0.1`, not
the Mac's own address.

#### Other programs on a Mac

Other programs of this machine, and other users, can connect to that
port. The macOS wrapper requires a fresh run-specific proxy secret for
`CONNECT` and plain HTTP requests, so knowing the port grants no internet
proxy access. Other users get no key and no bot: those routes take only the placeholder the
sandbox was given, a tag made under a secret random for each run and
holder, which nobody else can make.

#### Tools, Local Model, and Open

Nor can other users reach an agent's tools, its local model, or `open`: their URLs
(`LNK_TOOL_<NAME>`, `LNK_LOCAL_MODEL_<PORT>`, `LNK_SANDBOX_OPEN`, and
those the harness is set up with) carry a secret of the agent's, which
the proxy checks, and the proxy writes neither in its log. So no other
user asks you to open a page in the sandbox's name.

### Programs you run yourself

Programs you run outside a sandbox are not kept out. The placeholders, made anew at
each start, and the URL secret, made once, are written in the harness's
home and Link's settings, readable by your user. A program of yours
that reads one can use it while it holds, as it could read the key
itself in Link's settings.

#### Sandboxed Programs

Every sandbox hides agents' folders (`~/Link/Agents`, and
`~/Link/Archive`), `--files.read ~` or not, so a sandboxed program reads no
agent's placeholder but its own, or one in a folder you open to it with
`--files.read` or `--files.write`.

### What a web page can send

The proxy refuses what a web page sends, whatever its path: a request to the proxy itself
(not `CONNECT` or `http://...`) with a `Host` that isn't this machine's
loopback, which a name pointed here (DNS rebinding) carries, or one a
browser marks as from another site (`Origin`, `Sec-Fetch-Site`). For
another host's `Host`, its 403 says, as `Link's sandbox: ...`, which
host it refused and that a sealed route serves only its own host, since
a program that sets one by mistake needs to know; the others are a bare
403.

#### Refusals Before the Count

The proxy refuses such a request before it counts toward a pause or the calls a minute,
and logged as `browser` once a minute at most, so a page the user visits
can't pause an agent or use up its calls.

## Sockets and Reaching the Harness

An agent's sockets are in `<runtime>/lnk/agent/network/<agent id>/<harness>`
(700), in Link's settings folder, which the sandbox opens alone to the
harness. A run's, from [`lnk sandbox run`](../../../../../../docs/tooling/sandbox/guide.md#sandbox-guide), are in a temporary folder with a
random name, made by the run (700): no other user can make it first.

### Reverse Relay

Link reaches the harness for its health checks through it: Link listens
on the harness's ports on the box's loopback and passes connections in
through a second socket, as does a login's callback.

### Never through a link

The sandbox makes that socket in the folder it writes, so Link connects
to it without following a link, and only if it is a socket of the
user's: a link the sandbox puts in its place, to the session bus or
Docker's socket, reaches nothing.

## A blank proxy (Linux)

For a machine that packs agents, a proxy, its sandbox and its harness
can be started blank, holding nothing of anyone's, and bound to one
agent later.

### Started blank

`lnk sandbox proxy --blank --control <socket> --dir <folder>` (with
`--hold`, or a program after `--`) holds no rules, key, secret or wire.
It listens on its folder's `out.sock` and answers every call 503 (`this
proxy isn't bound to an agent yet`), so a sandbox behind it reaches
nothing at all. `--listen` and `--dns` aren't taken with `--blank`.

### Control Socket

`<socket>` is made 0600 and takes only its own user's connections, or,
with `--binder <uid>`, that one user's too, so a service running as
another user binds a proxy that isn't its own; every other user is
refused, root among them unless it's the one named.

### Bound by One Line

A blank proxy is bound by one line of JSON there, `link_sandbox_client::ProxyBind`:
what its command line takes, by the same names, but with its routes
(what each is given, a model's key and secrets among it), their caller's
key and the URL secret themselves in place of the files they'd be read
from, so none of them is ever written to the disk.

#### Answer to a Bind

A bind with routes names who calls them, or it's refused. It answers one
line, `{}` or `{"error"}`, and a bind that doesn't hold binds nothing.
From then on it's the proxy those arguments start. It's bound once:
another bind is refused, and there's no unbinding, since the binding
ends with the process.

### Bridge Reaching the Harness

A bridge reaches the harness through the sandbox's own `in.sock` in the
same folder, [its endpoint](../../../../../../../../../docs/more/modules/contracts.md) `unix:<folder>/in.sock:<port>`,
with no port on the machine's loopback.

#### As itself

The sandbox writes that folder, so the bridge reaches the socket as
itself: never through a link put in its place (Linux opens it without
following one and connects through the open file), only a socket owned
by the folder's owner, and only when what listens on it runs as that
user (its peer's credentials). A sandbox that swaps in a link to another
socket, or a socket of another user's, gets nothing: the connection is
refused before the proof is asked.

### Blank Sandbox Specification

The specification names no owner's path: an empty home of its own, the harness's files
read only, and the proxy's folder (`reached`). A test shows it reads
nothing of the folder beside its own, and that its only way out is that
proxy.

## The proxy as another user (Linux)

A proxy can run as a user other than its agent's, such as a user that
holds the machine's secrets and runs nothing else, so the agent's user
can't read what the proxy holds. The two share the socket folder alone,
by a group only they're in.

### Socket Folder

`lnk sandbox proxy --dir <folder> --agent-user <uid> --group <gid>`
(blank or not) starts only when `<folder>` is of group `<gid>`, mode
`2770`: theirs, closed to everyone else, and set-group-ID, so what's
made in it takes its group. It makes `out.sock` and `local.sock` there
`0660`, of that group.

### `in.sock`

The relay inside the sandbox makes `in.sock` `0660`; it can't name the
group from inside its user namespace, so the socket takes the folder's.
The proxy connects to `in.sock` only as a socket of `<uid>`'s, never
through a link: not its own user's, which a sandbox can't make.

### Folders a Sandbox Reaches

`lnk sandbox wrap` keeps a folder the sandbox reaches (`reached`) that
is its user's with mode `2770` as it is, and makes every other its
user's alone (`0700`). The proxies one sandbox reaches share their
folders alike, or it's refused.

### Nothing else is shared

The agent's home is its user's alone, and the proxy's wire and control
socket are the proxy's user's. A test packs two agents with their
proxies each a second user, and checks that each sandbox's way out still
works through its own proxy, that no user of one slot reaches anything
of the other's, and that a proxy's user reads nothing of its own agent's
but the socket folder.

## With no sandbox

An agent with no sandbox, started with [`lnk agent start --no-sandbox`](../../../../../../../../agents/agent/docs/agents/README.md#start-your-agent), keeps its
proxy. Its tools other than `run` and `file`, [which need a sandbox](../../../../../../../../agents/agent/docs/agents/tools/use/security.md#run-and-file),
its memory, its scheduler, its model's key and its sealed secrets added
outside, the wire's log, its counted usage, [`lnk sandbox stop`](../../../../../../docs/tooling/sandbox/guide.md#stop-a-sandbox) and
pausing don't depend on the wall, so they stay; only the wall goes.

### Unwalled

`lnk sandbox wrap` given `unwalled` (no wall, its proxy beside it) runs
the harness as it is under `lnk sandbox proxy --listen <port>`, on this
machine's loopback at the agent's own port (the last of its block, as on
a Mac), its routes behind its URL secret. The harness is pointed at it
(`HTTPS_PROXY`), and the proxy filters and logs what it's sent as the
agent's specification says. With `[network] mode = "host"` it's pointed at no
proxy, which then serves only Link's ways out.

### Advice, not a wall

The proxy is then advice the program may ignore. Nothing keeps the
harness to it: it reaches the internet, your network and this machine
directly, and reads and writes what you can, Link's settings among
them, where the keys its proxy adds are kept. Its hosts and endpoints
filter only what it sends its proxy, and `stop` closes only that.

### No route, and `harness run`

A route is refused with no sandbox: only a sandbox keeps a program to
its proxy, so with none, a route would hold nothing back. `lnk agent
start` says so. `lnk agent harness run` with no sandbox runs its command
beside a proxy of its own on a free port it holds and hands it open, as
on a Mac.

## Through a SOCKS proxy on a socket

A route can name a SOCKS proxy on a Unix socket of the machine,
`socks5://<path>`, such as one another machine offers with `ssh -R` or [`lnk box forward --socket`](../../../../../../../../compute/box/docs/services/boxes/ssh.md#forward-home-to-a-box). SOCKS is a protocol a
program speaks to a proxy, asking it to connect somewhere on its
behalf. The connection leaves from wherever that proxy is; the
harness's DNS lookups happen on the machine it runs on.

### Socket

Keep the socket in the machine's runtime folder, which every sandbox
hides: the agent can't reach it but through its proxy. Anything of your
user on the machine can use that SOCKS proxy while it's up.

### One round trip

The proxy sends the SOCKS greeting and its request at once, one round
trip to the SOCKS proxy. Of a host's addresses, the next is tried 1 s
after the one before through it, not the quarter second it waits when
connecting directly.

### Fails closed

With the SOCKS proxy gone, nothing answers on the socket and nothing
gets out. The proxy never falls back to the machine's own network.

### What stays direct

The harness's install and configuration, such as downloading a harness,
leave from the machine, through a proxy of their own limited to the
hosts the adapter lists, and `lnk` itself uses the machine's network.
Only the run's own traffic takes the route.

## Overlapping routes

When routes overlap, the most specific matching prefix wins. A typed
action can therefore coexist with its namespace’s native carrier.

## Read the Wire and the Decisions

Requests, responses, and refusals cross the proxy's boundary as
[the wire](wire.md) defines. The reasons for the proxy's filtering
choices are in [Decisions](decisions.md).

<!-- docs-tree:start -->

## Docs

### Pages

- [Where a Sandbox May Connect](connect.md): What the filtering proxy refuses, and how it reaches the hosts, endpoints and ports a sandbox may
- [Proxy Protocol](wire.md): What the filtering proxy logs of every call, and how its wire is stopped or paused

### More

- [Decisions](decisions.md): Why the sandbox's filtering proxy, `link-sandbox-proxy`, works the way it does

<!-- docs-tree:end -->
