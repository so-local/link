# Where a Sandbox May Connect

What [the filtering proxy](README.md) refuses, and how it reaches the hosts, endpoints
and ports a sandbox may.

## What the proxy refuses

It resolves each name itself and refuses the addresses below, and every
address assigned to the machine whatever its range, such as a server's
public address on its own interface. That covers the box, its network
and the cloud's metadata service, `169.254.169.254`, [but not a public
address](../../../../../../docs/tooling/sandbox/security/gaps.md#known-gaps) the cloud's NAT holds for the box.

### The addresses

Loopback, private, link-local, carrier-grade NAT, unique-local,
the retired site-local (`fec0::/10`), reserved (`240.0.0.0/4`),
benchmarking (`198.18.0.0/15`) and `192.0.0.0/24` addresses, and Azure's
host for its machines (`168.63.129.16`). Documentation ranges
(`192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`, `2001:db8::/32`
and `3fff::/20`) are never a real host, and Teredo (`2001::/32`) tunnels
to an IPv4 address written inside it, so both are refused too.

### IPv4 inside IPv6

IPv4 ones written inside IPv6 (mapped, compatible, NAT64
`64:ff9b::/96`, 6to4 `2002::/16`) are refused as the IPv4 address they
reach. `check` asks it for this machine, a metadata service and a
private address in each form, the compatible `::a.b.c.d` too.

### A name resolving there

A name resolving there is refused the same way, so it can't point the
exit at this computer either. It keeps a name's addresses for 30
seconds, and checks them on every connection.

### Time and size

A request's head must arrive within 30 seconds, and is at most 16 KiB.
A call that carries nothing either way for 10 minutes is ended: a tunnel
(`CONNECT`), a plain `http://` request, a hosted or local model's, or a
Telegram call.

### Connections at once

It serves at most 512 connections at once on each of its listeners; the
next waits for one to end, so a program can't use up the proxy's file
descriptors.

### Ports

`CONNECT` reaches port 443 only, and a plain `http://` request port 80
only, but for the endpoints a specification names (`host:port`, `lnk agent permission add
--internet.tcp`). A list of hosts still applies to them. A host is only a name
or an address.

### Calls a minute

With a limit, `per_minute`, which [`lnk agent permission add --internet.per_minute`](../../../../../../../../agents/agent/docs/tooling/sandbox/permissions/README.md#choose-what-your-agent-can-reach) sets, it takes at
most that many calls within a minute, of every verb, refused ones
counted too, and answers the
rest with a 429 and `Retry-After`. Those refusals are logged and count
toward no pause.

### A plain `http://` request

It goes to its host alone: its `Host` header is set to the host checked,
never the client's, the connection closes after one request, and a body
in chunks is refused. So a CDN that routes by `Host` can't be pointed at
another site past a list of hosts.

### The TLS ClientHello

Over `CONNECT` to port 443, with a list of hosts, the TLS ClientHello
must ask for the host checked (its SNI), or for none to a listed
address, which TLS names none of, or the tunnel is closed. An
encrypted ClientHello, or another `Host` inside the TLS, can still front
another of a CDN's sites, which only the CDN can stop.

## A request means one thing to every server

The proxy refuses a request head with a bare `\r` or `\n`, since every
line must end in `\r\n`, or any other control character but tab. What
follows a head is never read as its head. A local model's request with a
folded header, or two lengths, is refused too.

### Why

Some servers, such as Go's (which Ollama is) and Node's, end a line at a
bare `\n`, so a header the proxy checked could otherwise hide another,
or hide a second request, such as a pull behind a chat, that its filter
never saw. The parsers are fuzzed: what they pass on is read back as
such a server reads it.

## Hosts, when listed

With `[internet] hosts` (`--internet.hosts`, `lnk agent permission add
--internet.hosts`), the proxy refuses by name any host not listed, before
resolving it, with a 403 and no DNS query. Names are compared without
case or a final dot. `*.example.com` covers the names under it, not
`example.com`. An IP address is reached only if listed as one. Listed
names still resolve here, and still never reach this machine or its
networks.

## Named TCP endpoints and git over SSH

The endpoints a specification names (`host:port`) are reached through the proxy,
by ssh's ProxyCommand anywhere, and on Linux by an address of the
sandbox's own.

### Git over SSH

It gets out through `GIT_SSH_COMMAND`, whose ProxyCommand is
`lnk-sandbox connect`. It runs inside the sandbox and tunnels
through the proxy on its loopback, never anything else, once its host's
port 22 is named. For `ssh` itself, run `$GIT_SSH_COMMAND user@host`.

### An address of its own (Linux)

On Linux, a named host with a port from 1024 that its hosts allow also
gets an address of
its own in the sandbox, from `127.0.1.1` on, in the sandbox's own
`/etc/hosts`. The proxy's end in the sandbox listens there and sends
each connection to the proxy as `CONNECT host:port`, which the proxy
checks and logs as any other. A port below 1024, an IP address, or
macOS: only through a command such as ssh's ProxyCommand.

### Its `/etc/hosts`

That file is written in a private folder beside its sockets' folder
(`<folder>.etc`, which no other user can write or plant a link in), out
of its reach.

### Every name under a domain

An endpoint may hold every name under one domain on one port
(`*.abc.mongodb.net:27017`), never the domain itself; its hosts still
have to allow each name. `check` warns of one over a domain a provider
shares among its customers (`*.mongodb.net`, a cloud region's
`*.us-east-1.rds.amazonaws.com`), which reaches anyone's database.

## DNS where a sandbox names endpoints (Linux)

A sandbox whose specification names endpoints gets a resolver: its own
`/etc/resolv.conf` (in the same private folder as `/etc/hosts`) names
`127.0.0.1:53` in its network, where its proxy answers, outside. Every
other name is refused before anything is asked, so a query reaches only
the nameservers of names the sandbox may reach anyway.

### Listening there

The proxy has bubblewrap wait (`--block-fd`) until a helper,
`lnk-sandbox dns-socket`, joins the sandbox's network as the
owner of the user namespace bubblewrap made for it, binds
`127.0.0.1:53`, hands the sockets over and exits. It runs nothing in the
sandbox and reads nothing of it. Where joining fails, the sandbox runs
without DNS, and says so.

### A name an endpoint holds

One its hosts allow gets an address of the sandbox's own: its
`/etc/hosts` one, or one from `127.1.0.1` on for a name under a `*.`
endpoint, where the proxy's end in the sandbox listens on the endpoint's
ports (from 1024) on every loopback address. Such a name gets no IPv6
address.

### Connecting to it

A connection there goes to the proxy as `CONNECT` to that address, which
the proxy takes for the name it gave it, and checks as any.

### A name its hosts allow

It gets its public addresses, never this machine's, its network's or a
metadata service's; they reach nothing but through the proxy. A name
whose only address refused is a metadata service's pauses the wire, as
connecting to it does.

### SRV and TXT

SRV and TXT for such a name, or a service of it (`_mongodb._tcp.<name>`),
get what this machine's resolver answers, as records of that name only.
An SRV answer's targets are names the program then looks up and
connects to, each checked as any.

### Never a local network's names

One label, or under `.local`, `.localhost`, `.localdomain`, `.internal`,
`.intranet`, `.private`, `.corp`, `.home`, `.lan` or `.arpa`
(`home.arpa`, reverse lookups), which this machine's resolver answers
from its network (mDNS, a company's zone), nor those no public name has
(`.test`, `.example`, `.invalid`, `.alt`, `.onion`).

### A split view

With every host allowed, a name under a public domain that only your
network's nameserver answers (a company's split view of
`corp.example.com`, its `_ldap._tcp` records) still gets that answer.

### Which resolver

Every lookup, these and a name's addresses, goes to this machine's
resolver, never through the upstream: on a box routed through a SOCKS
proxy, the box's nameservers see the names, as they do for the proxy's
own.

## DNS Records and Limits

DNS observations are counted and retained separately from forwarded traffic.
The resolver applies explicit budgets and reads only its declared inputs.

### Logged and counted

Each query is a `dns` line in the wire's log. Over the limit it's
refused (`REFUSED`). A refused one is logged once a minute at most and
never counts toward a pause: looking up a name it may not reach gets the
program nothing.

### What counts

What asks outside counts toward its calls a minute: SRV, TXT, and an A
or AAAA query whose name's addresses aren't kept from a lookup in the
last 30 seconds, nor kept as having none from one in the last 5, nor
being looked up already; not an endpoint's own name, nor a refused one.

### Kept and shared

A name's addresses come from this machine's resolver as the proxy's
connections do, kept 30 seconds and shared with them; a name with none
is kept as such 5 seconds. A name is looked up once at a time: its A and
AAAA, and connections at once to it, wait for the first lookup's answer
rather than asking again.

### Its limits

At most 16 lookups that wait on a nameserver run at once, for 5 seconds
each (SRV and TXT, 10), so slow ones neither stall the proxy's own nor
hold up the sandbox's answers for its endpoints. It answers 256 queries
over UDP at once and drops the rest, and gives at most 65,000 names
under `*.` endpoints an address for as long as its proxy runs; a name
past them gets `SERVFAIL`.

### What it reads

Queries are read as plain DNS of one question; anything else gets an
error. An answer over UDP longer than 512 bytes is cut for the program
to ask again over TCP. The reader is fuzzed.

## The machine's own ports

The ports a specification names, such as a local model's, are reachable inside
at the same port, relayed through a third socket. The port is checked
outside, and no other port of the machine is reachable: every port
(`[localhost] ports = ["*"]`) is taken only with `[network] mode =
"host"`, never behind a proxy. A port or model a sandbox names gives it
its proxy, so on Linux the port is relayed in like any other.
