# Security

> Security Defines the Sandbox Guarantees

What every sandbox guarantees, what can cross it, and how a specification is
checked. What a harness's sandbox opens, grant by grant, is in [agents'
security](../../../../../agents/agent/docs/agents/more/security.md#permissions).

## Where each part is

Every sandbox starts closed, and what it closes on each OS, and how, is
the wall's: [Sandbox Isolation](../../../confine/docs/tooling/sandbox/wall/README.md).
Every sandboxed agent with `[internet]` runs behind the filtering proxy,
its one way out, unless it has `[network] mode = "host"`: [Filtering Proxy](../../../proxy/docs/tooling/sandbox/more/proxy/README.md). The
tools that run outside are on [Tools and Asking](tools/README.md), and what it
leaves open on [Gaps and Limits](security/gaps.md). What a sandbox can be opened to is on [Sandbox
Permissions](security/permission/README.md), and every call it made past
its walls, allowed or refused, on [Sandbox Log](security/log/README.md).

## What can cross the sandbox

A sandbox is only as closed as what it may talk to: a service it may
reach can be made to do more than it's for. So each way out is judged
by what it can be made to do, and `check` tries each one. A sandbox that
reaches any host (`[internet] hosts = ["*"]`, or `[network] mode =
"host"`) can send anything it can read anywhere.

### The network as it is

`[network] mode = "host"` (`--network.mode host`): any host and port on
the internet. On Linux also every localhost port and the machine's
abstract unix sockets, X11's among them. On macOS also every localhost
port through `::ffff:127.0.0.1`, and the Mac's own address. It writes
what any service it reaches keeps.

### `[internet]`

`--internet.hosts`: any host over HTTP `CONNECT` or plain `http://`, or
only those listed. Never this machine, its networks or a cloud's
metadata service. Agents with `[internet]` get it unless they have
`[network] mode = "host"`; on a box, always.

#### Web, beside `[internet]`

[Web](../../../../../agents/tools/web-tool/docs/agents/tools/web/guide.md),
the page-reading tool, runs outside the sandbox and fetches any public
host, whatever `[internet] hosts` lists, or with no `[internet]` at all.
It never reaches this machine, its networks, or a metadata service. So
an agent given web can send what it reads to any public host, through
the URLs it asks for. Leave web out of an agent whose `[internet]` is
narrow.

#### Temporary native names

The dotfile probes may temporarily create `.git`, `.envrc`, `.vscode` and `.idea` in writable folders, including those under Home. This is the visibility rule's exception: the native names are required to test whether confinement blocks them, and the check removes the probe files it creates.

#### On macOS

When it reaches the internet, the certificate checks (`trustd`) too,
which TLS through the Security framework needs end to end; never DNS or
the network's settings, which the proxy does outside.

### Models and a service's secret

A local model through the proxy: that runtime's inference only. Its
admin API gets a 403 from Link, and on macOS its port itself is closed.
A hosted model through the proxy: that provider's inference only, with
the key added outside. Its server-side tools are removed and URLs it
would fetch are refused. The provider keeps what it keeps of requests.

#### A service's secret

Through the proxy (a Telegram bot's token): that service's calls on the
paths its rule allows, with the secret added outside, but for those the
rule refuses (Telegram's `setWebhook`, `logOut` and `close`). A bot can
message any chat it can, so what it read can go to a chat of anyone's
choosing.

### `[lan]`

`--lan.hosts`, through the proxy: the machines of your network it lists,
on any port, by name, address or range, `*` for every private one: only
private addresses (10/8, 172.16/12, 192.168/16, fc00::/7), the
carrier-grade range (100.64/10, Tailscale's) only by an exact address.
Dialed from here, never through the upstream. Whatever serves on those
machines can be made to do what it does for anyone on your network.

#### Never, whatever is listed

This machine, its containers and VMs (the network of any bridge it has,
Docker's, libvirt's or a VM app's, of VirtualBox's, VMware's and
Parallels' host networks and QEMU's taps, and on Linux of any interface
with no device behind it), a link-local address or a cloud's metadata
service. A listed name that resolves outside your network reaches
nothing. On a cloud's machine, an EC2 Mac among them, `[lan]` lists
addresses rather than a whole private block.

#### Addresses Written Another Way

An IPv4 address written in IPv6 (mapped, compatible, NAT64,
6to4) or as a URL may write it (`3232235777`, `192.168.0x1.1`) is judged
as that address, and so is one under the network's own NAT64 prefix,
learned once from the IPv6 addresses its resolver gives `ipv4only.arpa`
(RFC 7050, a `/96` prefix). A final dot is no part of a name, so an
address written with one (`192.168.1.5.`) is read as the address before
any lookup.

#### With `*`

A name is dialed only at a private address this machine's resolver gives
it: a public one is refused, and where it resolves to nothing (a machine
without DNS, behind an HTTPS proxy alone) nothing is dialed. The
machine's own proxy (`HTTPS_PROXY`) is never the sandbox's upstream.

#### On a cloud's machine

On AWS, Google Cloud or Azure, by its firmware, `*`, or a range holding
a whole private block (`0.0.0.0/0`, `10.0.0.0/8`, `::/0`), is refused,
whoever made the specification: the private ranges there are the cloud's network.

### `[browser]` and a page opened

`--browser.hosts`: its hosts are apart from the internet's, so a sandbox
reaching one host may still ask to open a page on any host it lists. The
page carries what the URL holds, so each one is asked.

#### A page opened, through the proxy

An `https://` page in your browser, after you allow that one page, its
whole URL shown. It is asked for at `$LNK_SANDBOX_OPEN`, behind the
proxy's secret, which `$BROWSER` in the sandbox uses. The page opens
with your cookies and logins, so a URL can carry what the program read
to a site, or act as you on one you're signed in to: allow only a page
you asked for.

#### Never

An app, a file, another scheme, or this machine or its network by name
(`localhost`, `localhost.`), by a name that resolves there when it's
asked (`127.0.0.1.nip.io`), or by address, however a browser reads one
written (`127.1`, `2130706433`, `0x7f000001`, `0`).

#### Its name, as it resolves

The name is checked as it resolves here within 5 seconds: one that
doesn't resolve by then is let through, and the browser resolves the
name again, so one whose answer changes in between, or that resolves
only for the browser, is its own.

#### A login's callback (Linux)

For a page you allowed whose `redirect_uri` is a port on `localhost`,
that port on this machine is passed into the sandbox until the first
request arrives, or for five minutes at most. Meanwhile anything on this
machine that connects to it reaches the program. A port another program
already listens on, at `127.0.0.1` or at `::1`, is refused, so the
login's answer never goes there, however the browser takes `localhost`.

### Tools and ports

A tool at a URL: that server, called from outside with its headers,
each call asking you as any tool's does. A localhost port (`--localhost.ports`, or
a local model's without the proxy): whatever serves it, with its whole
API, a model runtime's pull, create and delete included. `check`
reports it as `open`.

#### Granted at each call

The sandbox's bridge reads an agent's tools and pages from what the
module that keeps it grants at that call, or a keeper that isn't a
module wrote for it while it's bound, never from the route its proxy
was started with, so [a grant taken back holds at the next call](../../../contract/route/docs/tooling/sandbox/more/contract/routes.md#what-mcp-and-open-reach-now).

#### Namespace membership

A namespace's host definition fixes its member servers and action names.
An MCP argument cannot add a member or change its address. Every call
checks the original provider's current approval policy, and a removed
provider or changed launch is refused before forwarding.

#### A Namespace's Caller

A namespace's call is its caller's. The bridge starts a namespace's
server for one sandbox's session and names that sandbox in its
environment (`LNK_SANDBOX_CALLER`), so pausing the sandbox stops the
call, and its question is asked as that sandbox, waiting only behind
that sandbox's own. A namespace with no caller named, a shared one among
them, serves only the owner's own command line (`--by owner`).

### Folders

Folders it writes (`--files.write`, a harness's home, your files folder):
files only. You, [`lnk storage`](../../../../../storage/storage/docs/services/storage/README.md#storage) and agent state-transfer readers read them, as
[What Link reads back](#what-link-reads-back-from-a-sandbox) says.
`--files.read ~` reads your whole home folder, never Link's settings
or the sockets daemons keep there, though on Linux [a socket elsewhere in
it can be connected to](security/gaps.md#on-linux-a-socket-in-a-folder-it-reads).

#### Build folders

Build folders are the only folders it writes whose programs it may
run. [A toolchain's builds are build folders](reach.md#build-folders), such as Rust's `target/`
beside `Cargo.toml`. So is
the folder an installer installs into, when the program wrapping the
sandbox names it for an installer that runs what it downloads. Each is
under a folder the sandbox writes and never a link. Every other folder
it writes, and `/tmp`, still runs nothing. The installer's build folder
is temporary, until Link fetches and checks the runtimes installers
download and publishes them read only before an install runs.

#### Hidden folders made first

The folders every sandbox hides inside the Link folder and `~/.lnk`,
such as `~/Link/Archive`, are made before it starts, empty and only yours, where they aren't there
yet. On Linux an empty folder mounted over each hides it, and only one
that exists can be mounted over, so one made after the sandbox started
would otherwise be seen.

#### Its CA file

The file `SSL_CERT_FILE` names, where the program inside is given that
variable, is read only and alone: never written, never its folder, and
only an absolute path to a regular file. A file reached through a link
another user owns is refused, as any grant is, and Link's settings stay
hidden even when it names one there. A folder `SSL_CERT_DIR` names is
never opened this way, since a folder a variable names may hold more
than certificates, such as a key beside its certificate.

### On macOS

Listening, behind its proxy or with no network: only on localhost and
only on the ports it serves (`serve`), never one it only reaches, its
proxy's or a model's, so it can't take another program's port while
that one is stopped, or `[::1]:P` beside a server on `127.0.0.1:P`; with
`[network] mode = "host"`, on any address and port.

#### System Folders

Of `/System`, only the system's own folders (`/System/Library`,
`/System/Cryptexes`, `/System/iOSSupport`, `/System/DriverKit`) and the
boot volume's cryptexes: never `/System/Volumes/Data`, a second path to
`/Users`. A sandbox's temp files are `/tmp/<its name>` and names that
start with it then `-`, `.` or `_`, never another sandbox's that starts
the same, and the locale and region are read from the system's
`Library` and your own alone. `--commands.developer_tools`: Xcode and its settings, read
only, and its programs and the Command Line Tools' may run.

#### Services every sandbox gets

Users and groups, the log, notifications and the temp folder's helper,
each only as far as it answers. It can write to the system log.

#### The machine's facts it reads

The hardware's and the OS's facts (`sysctl`), named one by one: never
the list of every process (`kern.proc.all`), another process's
arguments or environment (`kern.procargs2`), or the boot session's ID
(`kern.bootsessionuuid`). One process's own facts by its ID
(`kern.proc.pid.<id>`) stay open, since programs read their own there
and Seatbelt can't tell theirs from another's.

#### What `lnk sandbox learn` reads

Seatbelt's reports in the system log, the kernel's alone (process 0,
sender `Sandbox`): a line any other program writes in Seatbelt's words
is never read as a program a sandbox ran.

## An agent's ways out

A Link agent's harness has the doors above that its specification opens, and two
kinds beside, its only others.

### Its proxy

The internet, its model and its tools, as its grants say, [the keys and
secrets added outside](../../../proxy/docs/tooling/sandbox/more/proxy/README.md).

#### Observing wire state

Status reads the owner's stop and pause flags through the sandbox client
contract on every request, without launching a command or caching the answer.
The observer gains no authority to open a wire, read a secret or bypass
revocation. Proxy enforcement, in-flight cancellation and private flag writes
remain with the proxy and its commands.

### Its bridges

A connection's (messages in and out; the connection's token stays
outside), the way in (messages from its schedules, subagents,
background commands and teammates), and the store bridge (its
conversations' events and lease, for a specification that keeps them with a
driver; the driver and what it reaches stay outside).

#### As itself

[Each call to one](../../../../../agents/agent/docs/agents/more/interface/security.md#every-way-out-of-its-sandbox) is the agent's identity, the action and a proof made
with the agent's own key, so a harness acts there only as itself.

## Specifications and checks

A specification is a file, and a file can come from anyone, so `lnk sandbox run
--spec` checks what it opens before it runs, and `lnk sandbox check`
tries it from inside.

### What a specification can and can't open

A specification not run here before, or one that opens something else since,
prints what it opens and asks. `--yes` skips the question. A path a link
makes elsewhere is shown with where it really is.

#### What's remembered

Link's settings keep, in `~/.lnk/safe/sandboxes/seen.toml`, each specification's path
and a hash of its grants with every path resolved through links and
every program as `PATH` found it, so a change in what it opens or a link
moved under a folder it names asks again; a new name or comment doesn't.

#### Refused in a specification's file

With the flags beside it: what would open Link's settings or agents'
folders, let code the program writes run outside the sandbox (in your
home's dot folders, on your `PATH`, in the system's folders or a
project's dot files), widen the specification's own next run, or [reach other
programs' sockets](../../config/specs/guides/sandbox.md#refused-in-a-file).

#### Folders a run is given to write

`--with-fd`'s `write` are refused [as a specification's writes are](../../config/specs/guides/sandbox.md#refused-in-a-file). A given folder
inside an agent's would open what the sandbox hides, so it's refused.

#### Link's settings

They stay closed to every sandbox, specification or flags, to reading and
writing: a grant inside them, or a write of a folder holding them, is
refused. This includes both personal Safes, root's `/etc/lnk` and the system
catalogue's control state. Installed programs in the user's and
system's program roots can be granted for reading, and never writing.

#### Runtime Data

Runtime data has narrow exceptions checked after resolving paths. A harness
may receive its own home. Memory may write its service's `run/` and read its
generated handoff and storage sockets. Its agent records, generation cache
and service log remain closed, and handoffs and socket folders cannot be
granted for writing. The shared [file layout](../../../../../../packages/files/docs/more/libraries/files/files.md)
defines those paths.

#### `[localhost] ports = ["*"]`

It's refused: every port of this machine would include other sandboxes'
proxies, each another one's way out. So is `40781`, the port of
`main`'s proxy.

### `[commands]`

It limits which files run, by the kernel: Seatbelt on macOS, Landlock on
Linux (5.13 and newer, enabled). A finite effective allowlist and a specification
without `[commands]`, which grants no programs, both require Landlock.
Every grant names what it runs, so every sandbox has executable
restrictions. A toolchain's build folders (Rust's `target/`) are the one
place a sandbox runs programs it wrote itself.

#### A launch refused

Missing Landlock, a missing in-sandbox confiner or an unsupported
syscall-filter architecture refuses the launch, naming the missing
capability. A Landlock probe preserves the ABI on success and the OS
error on failure. An unavailable syscall may mean missing kernel
support or filtering by an outer runtime.

#### Checked again inside

The sandbox checks again inside and stops before the target runs if
creating or applying the ruleset fails. It never substitutes
unrestricted execution.

#### Entries refused

An entry in a folder it may write, or in `/tmp`, `/var/tmp`, `/dev/shm`
or the temp folder, is refused, and so is a folder holding one of those.
`lnk sandbox check --spec` tries a program it doesn't list, and each way
a program might run code around the list. How a refused program is
named: [Sandbox Isolation](../../../confine/docs/tooling/sandbox/wall/README.md#naming-a-refused-program).

#### Programs found on PATH

A command named without a path is resolved to its real executable at
discovery, including through user-owned package-manager links such as
Homebrew's. The confinement carries that path into grant validation and
wrapping, so changing the discovery link does not redirect the grant.
An explicit path remains subject to the wall's refusal of non-root-owned
links, as do file and folder grants.

### Checking a sandbox

`lnk sandbox check` runs probes inside a sandbox as the flags or specification
given would make it, against bait it sets up, and removes its bait
after.

- a file with a random secret in the home folder, in Link's settings
  folder, in `/var/tmp` and in each hidden folder there is (agents'
  folders among them); and a listing of `~/.ssh`
- a write into the home folder, and a write into each folder the specification
  may write, which must work
- a server on a random localhost port, tried as 127.0.0.1,
  ::ffff:127.0.0.1 and the machine's address
- `http://1.1.1.1`, or behind a proxy, a tunnel to it through the
  proxy, and asking the proxy for this machine, your network, a host
  not listed, and IPv4 written in IPv6's forms
- the folder `lnk-sandbox` is in, and each dot file that runs outside
  at the top of each folder it writes
- a program holding the secret in its environment (`ps -E` on macOS,
  `/proc/<pid>/environ` on Linux)
- each localhost port the specification allows: whatever serves it is reachable
  with its whole API, reported as a known gap

Each probe is a shell script, handed its bait and what it tries in
variables of the `LNK_PROBE_*` family (`LNK_PROBE_ARG`,
`LNK_PROBE_CANARY`), never on its command line. The program holding the secret in its
environment is a helper the check starts outside, with the bait in
`LNK_CHECK_SECRET`: a probe that reads it there has read another
program's environment.

#### On macOS

`open -a Finder`, a search for a throwaway item Link adds to the login
Keychain, and a lookup of each system service by name. Those every
sandbox gets and those a grant opens count as allowed; the clipboard,
the screen, Apple Events, the privacy database and core services never
do. `lnk-sandbox` looks them up inside.

#### `leaked`

Anything that gets through without a grant allowing it is `leaked`, and
the command fails. A host asked to be refused is reached when the proxy
answers anything but its refusal (a 502 is an address it took that
didn't answer), but a name with no address here only when the proxy
connects: its 502 then is the name not resolving, and nothing was
dialed.

#### Tests

Tests also `lnk sandbox run` a program that tries to read `~/.ssh`,
Link's settings and its caller's environment, write the home folder and
reach the network.

## What Link reads back from a sandbox

Link treats everything it reads back from a sandbox as hostile.

### A harness's log

[`lnk agent log show`](../../../../../agents/agent/docs/agents/start/log/README.md#read-the-log), kept outside its home: a regular file only (a link in
its place is refused), read from where Link recorded the start, at most
its last 4 MiB, shown without control characters. With `--follow`, a
line without an end comes in 64 KiB pieces.

### How to run the harness

What its adapter's `command` says, run in its home: only plain absolute
paths that neither are nor hold its home, your files folder, your home
folder, Link's settings, `/tmp` or a folder of other programs' sockets
(`/run`, `/var/run`, `$XDG_RUNTIME_DIR`), checked as written and
resolved.

#### Like a specification's

None it writes is the system's (`/usr`, `/opt`, `/etc`, ...) or is,
holds or is in a folder on your `PATH`. Service names that are only
names; no `HOME`, `TMPDIR`, `LD_*` or `DYLD_*`.

### What to move and back up

Agent state transfer and backup read from the adapter's `state` and
its home: paths under its home only, links carried as links. Each name
is opened from its folder's handle without following a link, so one
swapped in during the read isn't followed either.

### A move's stream and a backup

Agent state reception and restore read another machine's transfer stream
or the files folder's `Backups`: unpacked in a folder out of
every agent's reach. An entry with `..` is skipped, a leading `/`
dropped, and one reaching out through a link, or a hard link to a file
outside, refused. Links stay links, and are moved, never followed.

### Everything else

Your files folder, as [`lnk storage push`](../../../../../storage/storage/docs/services/storage/guide.md#back-up) reads it: links are skipped, files copied
as data. Requests to its proxy: names are resolved outside. A session's
folder: Link reads nothing there. The wire's log: written outside the
sandbox, names cut at 300 characters, shown without control characters.

## Ordinary-box identities

On an ordinary box each sandbox runs as a fresh non-sudo host user, with
its controller and proxy outside that identity. Private state views
preserve controller ownership and read-only permissions; cleanup and
replacement use durable identities and process handles. The root-owned
journal is the catalogue's `System::WorkerJournal`, under the sandbox's
system state. It is control, so a run cannot read or write it, even with
explicit flags naming that path or a writable parent. The
requirements, retained-file capability limit and maintenance procedure
are in [box
security](../../../../../compute/box/docs/services/boxes/lifecycle/on-a-box.md#working-users).

### Never root inside

The root launcher starts bubblewrap as root, and the sandbox's first
program drops every user and group ID to the worker's, clears every
capability and refuses gaining one back, before any program of the
worker's runs. What sudo adds to the launcher's environment
(`SUDO_USER`, `SUDO_UID`, `SUDO_GID`, `SUDO_COMMAND`) never reaches the
sandbox.

## Sessions over SSH

[Sandboxed SSH Sessions](more/sessions.md) defines the forced command
interface: whoever lets the key in (a box's root, for [a box's
users](../../../../../compute/box/docs/services/boxes/lifecycle/on-a-box.md#a-boxs-users))
names both on the line, where the key's holder can't change them. The
specification is read as any specification is, at each session, and the folder is the one
place it writes.

### In the sandbox

A shell, a command, `scp` or `sftp` all run in the sandbox: what the
session asked for (`SSH_ORIGINAL_COMMAND`) is run by `/bin/bash` inside
it, so a specification whose `[commands]` doesn't let `/bin/bash` run is refused.
`--check` runs nothing: it says what the sandbox would open.

### Its terminal

On Linux a session with a terminal keeps it: bubblewrap's new session is
for a terminal someone else reads after, and theirs is their own. `scp`,
`sftp` or a command without one gets a new session.

#### A Terminal on macOS

On macOS a run that doesn't own your terminal (its input isn't one, or
it isn't in the terminal's foreground) starts in a session of its own,
with no controlling terminal, so it can't type into your shell
(`TIOCSTI`), which Seatbelt doesn't stop. A run in the terminal's
foreground keeps it, and can.

### What it reads

A session reads nothing of the home folder it runs with but what its
specification opens, never Link's settings or an agent's folder (`~/Link/Agents`,
`~/Link/Archive`), even with `--files.read ~`, and has no network unless its
specification opens one.

## Backend Guarantees

[Features](security/features.md) compares the available confinement
mechanisms and the guarantees supplied by each supported backend.
