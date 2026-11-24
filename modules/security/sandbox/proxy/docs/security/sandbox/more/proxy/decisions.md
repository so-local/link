# Decisions

Why [the sandbox's filtering proxy](README.md), `link-sandbox-proxy`, works the way
it does.

## One wire out

Why a sandbox has one way out, its proxy, and what the proxy refuses on
it.

### Why is every way out a route on one wire?

A sandbox is only as closed as what it's allowed to talk to. A hole left
open on purpose, to a package mirror, a local API or an app launcher,
turns a bug in that service into the way out, and a place to write to
into a place to persist. So a sandbox gets one wire out, its proxy, and
each thing it may do outside is a route on it, served by small code
of Link's, never raw access to a service: `chat` through the proxy
rather than a tunnel to a model runtime's whole API.

### Paths on one listener

Routes are HTTP
paths on the proxy's one listener (`/_link/model`,
`/_link/url/<secret>/open`, `/_link/url/<secret>/mcp/<name>`), since harnesses already speak HTTP to a base URL,
and one listener keeps one log and one set of limits. Each route is off
unless the specification names it, has no admin form (nothing that installs,
deletes or configures a service outside), fails closed, and has its
parser fuzzed.


### No route of its own

The proxy itself (`link-sandbox-proxy`) names no route. Each is one in the
one bind its caller hands it (`link_sandbox_route::Route`), mounted at a
prefix of `/_link/`: the proxy matches a request's target to a route,
refuses it without the URL secret if the route needs it, and hands it,
proved as the agent that calls, to the route's bridge outside, of the
module that owns what the route does (a model's to the models module,
a secret's to the secret module, tools and pages to the sandbox's), and
anything else at `/_link/` is refused.

### Its verdict

The bridge says its verdict,
which the proxy logs by the route's name, then answers. A route's
bridge reads its request's head with the one parser
(`link_sandbox_route::http`), so no two readers take one head two ways.
A new way out is a route and its module's bridge, never a change to the
proxy's loop.

### Why does a sandbox get the filtering proxy instead of the network?

Any sandbox can have the proxy instead of the network as it is
(`[network] mode = "host"`). On Linux it gets a
network of its own; on macOS the proxy's port is the only one Seatbelt
allows. The proxy refuses this machine by any of its addresses, its
networks and a cloud's metadata service. It tells the machine's
addresses by binding, whatever the range, since a server's public
address on its interface is the machine too. The specification's localhost
ports are relayed in, so a local model still works.

### Why are hosts refused by name, before any lookup?

A name not on the list gets a 403 and no DNS query, so a refused name
leaks nothing. `*.example.com` covers only the names under it. Listed
names still resolve here, and still never reach this machine.

### Why is a request head with a bare line ending refused, not cleaned?

A bare `\r` or `\n`, or another control character, means the proxy and
a server that ends a line at a bare `\n`, such as Go's or Node's, could
see different requests. Refusing costs nothing, since real clients end
lines with `\r\n`, and cleaning would be one more parser to get right.
The parsers are fuzzed with `proptest` in `cargo test`, reading what
goes out as such a server would.

### Why does the proxy read a tunnel's ClientHello?

A CDN routes a TLS connection by the name in its ClientHello, so a
tunnel to a listed host behind a CDN could ask for another of its
sites. With a list of hosts, the proxy reads the ClientHello of each
tunnel to port 443 with rustls, which it already uses, and closes a
tunnel that asks for another name, or none. Other ports aren't read:
there the server may speak first. An encrypted ClientHello still hides
the name inside, which only terminating TLS would show, and a sandbox
would then have to trust Link's own certificate.

### Why does `[lan]` go through the proxy?

`[network] mode = "host"` is the network as it is, this machine and
its ports included. A specification's `[lan]` goes through the proxy instead, so
a sandbox reaches only the machines it lists, never this one or a
cloud's metadata service, and every call is on its wire's log.

## Loopback Permissions

Loopback services belong to the machine outside the sandbox. Their permission
checks preserve that boundary even when a run names selected local ports.

### Why is `[localhost] ports = ["*"]` refused?

On this machine's loopback listen other sandboxes' proxies, each
another agent's way out with its own hosts, keys and exit. Every port
would include them, so a specification names its ports.

### Why does a sandbox with only `[localhost] ports` reach them on Linux too?

A specification's named ports are reachable independently of other network grants.
On Linux the loopback relay provides access only to those ports; on macOS
Seatbelt grants the corresponding local access. No general network access
is required. Settings migration reports the ports it carries into a specification.

## Endpoint DNS

Named endpoints need DNS answers inside the sandbox while the proxy retains
authority over which destinations those names can reach.

### DNS for named endpoints

Why a sandbox gets DNS only beside endpoints it names, and how its
proxy answers.

### Why does a sandbox get DNS only where it names endpoints?

A program that uses `HTTPS_PROXY` never looks a name up: the proxy
does. A database driver does, and `mongodb+srv://` asks for SRV and TXT
records before it connects. So DNS comes with `tcp`, the endpoints a
database needs, and a sandbox without them keeps having none, as before.

### Only names it may reach

Its proxy answers, so only names the sandbox may reach are asked of
anyone, and each query is on the wire's log. SRV and TXT for a local
network's names (`.local`, one label) are refused: this machine's
resolver answers them from that network, which the sandbox never
reaches. A query counts toward the calls a minute only when it asks
outside, once a lookup, since the connection after it counts too.

### Why do an endpoint's names get addresses of the sandbox's own?

The sandbox has no route out, so a name's real address reaches nothing.
An address of its own, on its loopback, is where the proxy's end inside
listens, and a connection there reaches the proxy as `CONNECT` to the
name, checked and logged as any. A name under a `*.` endpoint isn't
known until it's looked up, so its address is given then, and the
proxy keeps which name it gave it.

### Why does the proxy join the sandbox's network to listen on port 53?

Resolvers ask port 53, which only a privileged process may listen on,
and bubblewrap keeps every privilege from the sandbox. Its user
namespace is the user's own, so a helper of the proxy's, outside the
sandbox, may join its network, listen on `127.0.0.1:53` and hand the
sockets to the proxy, while bubblewrap waits to start the program.
Nothing in the sandbox gains a privilege, and nothing of it runs in the
helper.

## Models and secrets on the wire

How the proxy filters a local model and adds a hosted model's key or a
service's secret outside the sandbox.

### How is a local model filtered through the proxy?

Outside the sandbox, at the runtime's own address. The proxy's
`local-model` route hands each request to the models module's bridge,
which reads it and passes only inference, by exact method and path. An encoded or relative path is refused, not
decoded. It takes one request per connection so each one is read. The
program keeps `localhost:<port>`, so no harness needs a Link-specific
setting. It is an allow-list, not a deny-list: a runtime's new admin
endpoint is refused until it's known.


### On macOS

On macOS, where Seatbelt allows a port whole or not at all, the
runtime's port is closed and the program reaches the runtime on its
proxy at `/_link/url/<secret>/here/<port>`, through the same filter. An agent's
harness is set up with that address, on a proxy port fixed per agent,
the last of its block (`main`'s is `40781`), so it's known before the
harness starts.

### Why does the proxy add a hosted model's key, rather than the program?

The program speaks plain HTTP to a path on its proxy, `/_link/model`,
and the proxy speaks TLS to the provider. The alternative, a
certificate authority of Link's trusted inside the sandbox to read its
TLS, would be one more secret to protect; this way only providers the
models module knows get a key. It is a path on the proxy's own listener, not a
port of its own: one way out, one set of limits.

### Why are a provider's server-side tools removed?

A provider's web search, web fetch, code execution and MCP servers
reach the internet for the program, so a program limited to its model
would get around its hosts through them. They are removed by
allow-list unless the specification allows them (`server_tools`): tools the
program runs itself, a function or Anthropic's bash, text editor,
computer and memory, stay, and any other type goes. A URL the provider
would fetch, such as an image or a document, is refused rather than
removed, so the program learns why.

## Placeholder Authority

A placeholder identifies one allowed credential use. Its creation, lifetime,
and binding keep it from becoming a reusable credential outside that run.

### Why is a placeholder random for each run, and not a hash of its secret?

A hash of the secret is the same wherever it's given: one read from a
harness's home works for every agent holding that key, for good, and
what runs can test guesses at a short sealed value against it. A
placeholder is an HMAC-SHA256 of what it stands for, under a secret
made for each run and holder (`link_sandbox_route::Placeholders`),
so it says nothing of the secret, and one seen is no use to another
agent or after the next start.

### Who makes it

Whoever makes the confinement makes it and
hands it to the proxy beside the secret; the proxy refuses one that
isn't Link's shape, so an empty or guessable placeholder adds nothing.

### Across a start

An agent keeps its run's secret in its settings, so its harness's
configuration, the run itself and its proxy agree until it starts
again. A proxy kept running across a start without being given the new
placeholders (one restarted from its old files, a blank proxy never
bound again) holds the old ones, so it refuses the harness's calls: a
mismatch fails closed, never open.

### How does the proxy add a Telegram bot's token?

The harness receives a placeholder and the proxy's Bot API address. The
proxy substitutes the real token into the request path and establishes
TLS with Telegram, keeping the token out of the harness's logs and errors.
Calls pass through except `setWebhook`, `logOut` and `close`, which could
hand updates or the bot to another program. Slack and Discord clients
that maintain their own authenticated WebSockets still hold their tokens.

### Why is a secret's rule data from the module that owns the service?

The proxy applies one secret operation, `/_link/secret/<name>/`. The
service's module supplies the host, allowed path shapes, token placement
and refused paths through its `rule --json` answer. The harness passes
that rule to the proxy. The sandbox implements confinement without
implementing each service's API. A method containing a dot or slash
cannot bypass a refused method name.

### Why do a tool's and a local model's URLs carry a secret?

On macOS the proxy listens on the machine's loopback, where any program
and any user of the Mac can connect. The key and bot routes take only
the placeholder the sandbox was given, but a tool or a local model has
no key to tag, so their URLs carry a secret of the agent's instead:
`/_link/url/<secret>/mcp/<name>`, `/_link/url/<secret>/here/<port>`. The
harness module keeps it in the agent's settings, so the harness's
configuration and its proxy agree across starts; [`lnk sandbox run`](../../../../../../docs/security/sandbox/guide.md#sandbox-guide)
makes one per run.

### In the path

It's in the path, which every client keeps from a
base URL, compared in constant time and never logged. Linux takes the
same URLs, so one form works everywhere.

### Why does the proxy refuse models that search by nature?

Some models search the web whatever a request asks, so removing tools
can't stop them. With a list of hosts, the model route refuses them by
name, from the models module's table of providers; without one, the
sandbox reaches the web itself anyway. The harness, which knows the
model, says so when the agent starts, and that the proxy can't see a
key it holds itself.

## Limits, the log and the kill switch

Why a wire takes a limit, what its log keeps, and how it's stopped or
pauses itself.

### Why does a wire take a limit of calls a minute?

A pause stops a wire that looks wrong; a limit caps one that looks
right but does too much, such as a loop calling a model or a tool. It
counts every call on the wire, of every verb, in the last minute, and
answers the one over it with a 429 and when to try again, which clients
already handle. Those refusals count toward no pause: the program is
doing what its specification allows, only slower. It's off unless a specification
sets it, since no one number suits every agent.

### What does the wire's log keep, and where?

Every call on a proxy, as a host and port, never a path or body: a
path can carry what an agent sends out, and the log is for seeing where
it went. It writes a line when a call is allowed and another when it
ends, so a connection held open for hours is in the log from its
start. It lives in Link's settings, which no sandbox reads or writes.
`wrap` gives the proxy the folder, since the proxy runs with the
program's environment and a harness's `HOME` is its own folder.

### How does the kill switch work?

It is a file in Link's settings that every proxy looks at, for each
call and every second while one is open. There is no process to reach,
no signal to send, and nothing a sandbox can undo. `stop` then stops
the agents, since a sandbox with `[network] mode = "host"` has no
proxy to close: each
agent's start registers [`lnk agent stop`](../../../../../../../../agents/agent/docs/agents/harnesses/use/README.md#one-running-in-a-terminal) for it (`lnk sandbox on-stop`),
and `stop` runs what's registered. The sandbox names no harness, and
anything else running sandboxed in the background can register the
same way.

### What happens when a wire's calls look wrong?

Its proxy pauses that sandbox's wire and tells you. It never stops the
program silently or only logs it: the program keeps running with no way
out, and you look at the log and resume it. Only patterns no normal
work makes count: a burst of refusals, many targets tried in turn, a
cloud's metadata service. The thresholds are loose, since pausing a
busy agent that did nothing wrong costs you its work. The proxy writes
the pause outside, so nothing in the sandbox can undo it.

### How does a wire say it's paused on a box?

[`lnk agent show`](../../../../../../../../agents/agent/docs/agents/models/agent/README.md#set-its-model-and-how-hard-it-thinks) asks [`lnk sandbox list --paused --json`](../../../../../../docs/security/sandbox/guide.md#stop-a-sandbox), and shows its
harness's wire closed and why, as `wire` in `--json` too. On a box
there's no desktop to show a notification, and `status` is where you
look at your agent first. The pause itself is a file only Link writes, so reading it is all
`status` needs.

## Its port

How a proxy on this machine's loopback takes its port, and keeps the
way out to its own run.

### A loopback proxy owns its port, and takes a way out only with its run's secret

A proxy that needs a dynamic loopback port binds port zero itself, then
fills its child's generated URLs and, on macOS, writes that run's own
Seatbelt profile; briefly opening and closing a listener reserves
nothing. On macOS, and for a program with no wall, that port is on the
machine's loopback, and a process that left its run's group outlives
the proxy, so a later run's proxy may get the same port.

### The run's secret

Each wrap
makes a new secret, only in `HTTP_PROXY` and `HTTPS_PROXY`
(`http://lnk:<secret>@127.0.0.1:<port>`), where the proxy reads it
too, so a service runs it again as it is; it answers a `CONNECT` or
plain `http://` request without it `407`. A program with no wall not
pointed at it reaches the network itself, and gets none. A route keeps
its agent's URL secret. A Linux sandbox's proxy is in a network no
later run shares and takes none, so Java's clients (signal-cli) work.
