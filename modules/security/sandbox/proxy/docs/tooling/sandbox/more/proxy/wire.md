# Proxy Protocol

What [the filtering proxy](README.md) logs of every call, and how its wire is stopped
or paused.

## The wire's log

[`lnk sandbox log show`](../../../../../../docs/tooling/sandbox/security/log/README.md#see-every-call-on-the-wire) reads `~/.lnk/safe/sandboxes/wire.log` (600, in Link's
settings, which no sandbox reads or writes). Every proxy writes a line
per call: the time, the sandbox's name, the verb, the host and port (or
the provider, or the machine's port), and whether it was allowed,
refused or failed. When an allowed call ends, another line gives the
bytes each way.

- The verbs of a call out are `connect` (HTTPS), `tcp` (another port),
  `http`, `mcp` (a tool's call), `model` (hosted), `local-model`,
  `secret` (a service's call, its secret added, such as a Telegram
  bot's) and `port` (this machine's, Linux).
- The others are `dns` (a name looked up, Linux, where it names
  endpoints), `open` (a page in your browser), `callback` (a login's
  answer let in, Linux), `browser` (a web page's request, refused, once
  a minute at most), `pause` and `unknown`.
- Never a path, a header or a body.
- The names in it are what a sandbox asked for: cut at 300 characters,
  and shown without control characters.

### Its size

Past 8 MB it becomes `wire.log.1`, the older ones move up to
`wire.log.4` and the oldest is dropped, so 40 MB of every sandbox's
calls in all.

### Refusals it leaves out

A wire refused while paused, stopped or over its limit logs that once a
minute, with how many it left out, so a sandbox can't push its own
history, or other agents', out with refusals that cost it nothing.

### 120 lines a minute

Whatever its calls end in, a wire logs at most 120 lines a minute of
each outcome: allowed, ended, refused, failed. It leaves out the rest,
and its next line of that outcome says how many (`and 880 more failed
since`). A flood of calls that fail, or that go through, fills the log
no faster than that, and never hides another outcome's first lines.
What it leaves out still counts toward a pause.

### Where it is, and what it holds

Link tells each proxy where the log is, since the proxy runs with the
program's environment, whose `HOME` is a harness's own folder. It holds
a proxy's calls, not what a program does directly: one with `[network]
mode = "host"`, or with no sandbox, reaches the network without its
proxy.

### What each proxy carried

Beside it, each running proxy writes what it has carried, two byte
counts (a refused call's request among them) and its sandbox's name, to `traffic/<pid>.json`, every 5 seconds
while they change, which [`lnk sandbox show`](../../../../../../docs/tooling/sandbox/guide.md#see-what-each-sandbox-sent) reads.

### Whose it is

The file names when its proxy started, so a later process given the
same number isn't taken for it, and the proxy removes it when its
program ends; one killed outright leaves it until `lnk sandbox show`
finds it gone.

### Calls still open

When its program exits, a proxy logs each call it still had open as
`ended`, with the bytes it carried. A proxy killed outright, not
stopped, writes no `ended` line for them.

## The kill switch and pausing

Nothing inside a sandbox can write the files that stop or pause it, or
undo them. Pausing narrows an agent attacking what the internet
reaches, and brings it to light; it doesn't prevent it.

### `lnk sandbox stop`

While `~/.lnk/safe/sandboxes/stopped` exists, every proxy refuses every call
with a 403 and ends the calls it has open within a second. Each proxy
looks at the file itself, for each call and every second while one is
open, so no sandbox can hold it up or undo it. [`lnk sandbox start`](../../../../../../docs/tooling/sandbox/guide.md#stop-a-sandbox) removes
the file; agents stay stopped until started.

### What runs with it

Then `stop` runs what asked to stop with it (`lnk sandbox on-stop`,
kept in Link's settings, which no sandbox reaches): [`lnk agent stop`](../../../../../../../../agents/agent/docs/agents/harnesses/use/README.md#one-running-in-a-terminal)
for each agent here, which every agent's start registers, since an
agent with `[network] mode = "host"` has no proxy.

### What it doesn't close

`stop` closes the proxies, not a sandbox's direct network (an agent's
or a specification's `[network] mode = "host"`): those stop with their program.

### `lnk sandbox stop <name>`

For a sandbox running or one that has run, while
`~/.lnk/safe/sandboxes/paused/<name>` exists (a run's goes when it ends), that
sandbox's proxy
refuses every call and ends those open, as with the kill switch, until
[`lnk sandbox start <name>`](../../../../../../docs/tooling/sandbox/guide.md#stop-a-sandbox) removes it.

### A proxy pausing itself

Each proxy watches its own calls and writes that file on a pattern no
normal work makes: 30 refusals within a minute, 20 different targets
refused within 10 minutes, or a call to a cloud's metadata service
(`169.254.169.254`, `metadata.google.internal`), however it's written:
by the address a name or number resolves to.

### Said

It logs a `pause` line and shows a notification where it can. Where it
can't, on a box, [`lnk agent show`](../../../../../../../../agents/agent/docs/agents/models/agent/README.md#set-its-model-and-how-hard-it-thinks) and [`lnk sandbox list --paused`](../../../../../../docs/tooling/sandbox/guide.md#stop-a-sandbox) say so.

### Never failing open

A proxy tells it's paused by the file being there, even if it can't
read why, and one it couldn't write (out of file descriptors, say)
holds in the proxy until it's written. A proxy has a limit of
connections on each listener, and a listener that fails to accept one
waits a moment and goes on, so a program can't use up the proxy's
descriptors or stop its listeners.
