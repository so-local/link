# Gaps and Limits

[What the sandbox leaves open](../security.md), what `check` reports as `open`, and where
it can't run.

## Known gaps

`check` reports these as `open`. Behind its proxy instead, the first two
are closed and the others remain.

### On macOS, with `[network] mode = "host"`

Every localhost port, through `::ffff:127.0.0.1`, IPv4 localhost through
an IPv6 socket. Seatbelt doesn't count it as localhost, so the rule's
"any address" covers it, and a rule names only `localhost` or any host,
with `(remote ip6 ...)` matching IPv4 too, so Seatbelt can't close it.
Also the Mac's own LAN address, which isn't localhost to Seatbelt
either: a server listening on every address is reachable through it.

### On macOS, a run in the terminal's foreground

A run that owns your terminal, its input one and in the terminal's
foreground, keeps it as its controlling terminal, and Seatbelt lets it
type into your shell there (`TIOCSTI`). A run that doesn't own it starts
in a session of its own and can't.

### On Linux, with `[network] mode = "host"`

Every localhost port, since bubblewrap can't tell ports apart and the
sandbox shares the machine's network. And, where the kernel's Landlock
is older than ABI 6, the machine's abstract unix sockets: X11's
(`@/tmp/.X11-unix/X0`, which an X server may trust for the same user
without a cookie, to read the screen and type keys) and some session
buses'. ABI 6 and newer scopes them shut.

### On Linux, a socket in a folder it reads

A folder a sandbox reads is mounted read only, but a socket in it can
still be connected to. So the folders in your home where daemons keep
their sockets are covered wherever a sandbox reads them: Docker
Desktop's, Colima's, Lima's, OrbStack's, Rancher Desktop's and
1Password's are empty inside, and each socket in `~/.ssh` and
`~/.gnupg` (an SSH connection's shared one, a key holder's) is an empty
file.

#### What stays open

A socket in any other folder it reads, and one made in `~/.ssh` or
`~/.gnupg` after the sandbox started (an SSH connection opened later),
can still be connected to: a sandbox reading your home folder talks to
whatever serves it. Folders of sockets Link knows (`/run`, `/tmp`,
`$XDG_RUNTIME_DIR`) are refused in a grant and empty inside.

### A localhost port its specification names

A port named with `--localhost.ports`, or a harness's own ports, is open to that
service's whole API, when something serves it.

### On Linux other than x86_64 and arm64

No seccomp filter: a sandbox's first program still closes every
descriptor it was handed, but the system calls the filter refuses
elsewhere are open to the program, and a `[commands]` list is refused,
since nothing there holds it.

### On macOS, a descriptor left open

A file descriptor what started the sandbox left open: the program keeps
it, and Seatbelt doesn't filter writing to a pipe or socket it already
holds.

### On macOS, a socket in a folder a sandbox writes

A socket Link makes there (an agent's store bridge's) is bound by its
path in a folder of its own beside it: a sandbox swapping a link in for
that folder as it's made has the socket made, a new file named `s`,
wherever the link points that you can write. Nothing more: it's made
private, given, moved and removed through the folder Link holds open,
never through the link.

#### Its mode and owner

They're set only once it's checked a socket of yours with one name, so a
hard link swapped in for it is refused; on macOS that check is by its
name just before, so a swap in between can still have a file of yours
made private or given as the socket would be. On Linux it's bound,
checked and changed through handles alone.

### On Linux, a missing `.envrc`

It isn't held: a sandbox can make one. direnv runs a new or changed
`.envrc` only after `direnv allow`, which shows it.

### On Linux, code run around `[commands]`

Where a specification lists programs, the program loader, which every program
needs, runs any file the sandbox can read (`ld.so ./program`): it has to
stay runnable for every dynamically linked program, so it can't be
closed. And a listed program loads a library the sandbox wrote when
`LD_PRELOAD` names it: the variable is cleared when the sandbox starts,
but a program can set it for a child it runs.

## What `check` doesn't report

These are open too, and `check` doesn't try them.

### `[commands]` limits files, not code

A listed interpreter (`sh`, `node`, `python3`) runs any script it's
given, and on macOS a listed program that isn't Apple's loads a library
the sandbox wrote when `DYLD_INSERT_LIBRARIES` names it (cleared when
the sandbox starts, but a program can set it for a child).

#### What is closed

On Linux, running any file descriptor (`execveat` with `AT_EMPTY_PATH`)
is closed where a specification lists programs. Memory files are either sealed
non-executable or their creation is refused when sealing or its
listener is unavailable. `check` requires those paths to be blocked.

### Dot files below the top

A project's dot files that run outside are read only at the top of each
folder a sandbox writes, and nowhere else. A sandbox can make a `.git`
in any folder inside (`web/.git`, with a `core.fsmonitor`), and git run
there, by you or by a shell prompt that shows git's status when you `cd`
in, runs what it wrote, outside.

#### A repository of its own

A repository it clones or makes in a folder inside is its own, hooks and
settings: running git there runs what it wrote. A folder holding your
projects (`--files.write ~/code`) keeps none of theirs. Where
`--files.write_dot_files` names a folder, it writes them all. A script of the
project that your git settings already run (a filter's, an alias's) is
the project's, as its Makefile is.

#### A hook manager's settings

Those that aren't there (`.pre-commit-config.yaml`, `lefthook.yml`)
aren't held: a sandbox can make one. A hook manager runs one only where
its stub is in `.git/hooks`, which is read only, and its install makes
the settings when there are none.

#### The placeholders, on Linux

Removing one of the empty placeholders made in a dot file's place while
a sandbox runs (`git clean -d` does) lets that sandbox make it. A
sandbox killed outright leaves its placeholders until the next sandbox
on the folder ends.

#### Link's data folders, on Linux

In `~/Link/Agents` and `~/Link/Teams` no placeholder is made, so a
program can make a `.git`, `.envrc`, `.vscode` or `.idea` there. It
stays until the program's run ends, or until the next run to end when
the program's was killed outright. Git or an editor run there meanwhile
[runs what it wrote](../../../../confine/docs/tooling/sandbox/wall/decisions.md#why-are-links-data-folders-swept-rather-than-held).

### On macOS, an `[internet] tcp` endpoint

It's reached only by a client that connects through a command, as ssh's
`ProxyCommand` does (git over SSH). A database client such as `psql`, or
a driver, can't reach one there: with no network of its own, a name
can't be given an address of the sandbox's, and Seatbelt can't send a
connection elsewhere.

### On macOS, calls to its proxy

Any program or user of the machine can send the proxy calls without a
browser's marks. A way out (`CONNECT`, `http://...`) without the run's
proxy secret, and a request for a route without its URL secret, are an
outsider's, refused before they count: they never pause the wire or use
its calls a minute, and the log keeps them apart, once a minute. What
holds the secrets counts as the sandbox's: 30 refusals in a minute pause
its wire until `lnk sandbox start <name>`, and its calls use up its
calls a minute and its 512 connections.

### A port it serves, for a server that speaks first

A connection to a port the sandbox serves is held only once it says
something within 10 seconds, so connections that say nothing never fill
the sandbox's. A protocol whose server speaks first (SSH, SMTP, a
database's) isn't served through it.

### A hosted model that searches by nature

Such as Perplexity's `sonar` models or OpenAI's `*-search-preview` on
OpenRouter, it fetches pages for the sandbox whatever its route removes.
With a list of hosts and the key on the wire, [the route refuses it](../../../../contract/route/docs/tooling/sandbox/more/contract/models-and-secrets.md#a-hosted-models-key-added-by-the-proxy);
with the key in the harness, it can't see it, and [`lnk agent start`](../../../../../../agents/agent/docs/agents/README.md#start-your-agent)
says so. Name a model that doesn't.

#### A URL under another key

The model route knows the ways a provider takes a URL to fetch by their
keys (any ending in `url` or `_data`); a provider's new way under
another name isn't refused until Link knows it.

### A port `[localhost] ports` names

`--localhost.ports` is reached whatever listens on it, Link's own among them: on
macOS another agent's proxy (each agent's on a fixed port), or a
sandbox's proxy on a free port; an agent's harness served on this
machine's loopback (its gateway); a model [`lnk model serve`](../../../../../../agents/model/docs/agents/models/local.md#serve-one-on-this-machine) serves.

#### Only `40781`

The sandbox can't tell those ports from others without reading other
modules' settings, so only `40781` is refused: name only ports you know.

### A public address not on an interface

The proxy doesn't refuse it. On a cloud box behind the cloud's NAT (AWS,
Google Cloud), its public IPv4 is such an address, so a sandbox there
reaches the box's own services on it where the box's firewall lets the
internet in. So does a home router's public address, where the router
answers from inside. Keep a box's firewall closed to what shouldn't be
public.

## Limits

Where the sandbox runs, and what doesn't work inside it.

### macOS and Linux only

On Linux it needs bubblewrap, and user namespaces the kernel allows, which [Ubuntu's own rule](../../../../confine/docs/tooling/sandbox/wall/README.md#ubuntus-rule-for-user-namespaces) may restrict.

### On Linux

Debuggers and tracers (`gdb`, `strace`) don't work inside a sandbox,
since `ptrace` is refused. Programs that need io_uring use their
fallback, and nothing inside can make a container of its own: Docker,
Podman, or bubblewrap again.

#### Run by root

What needs one of root's capabilities fails inside, such as listening
on a port below 1024, and a folder it's given that another user owns (a
project mounted as uid 1000 in a container run as root) can't be
written: its user namespace maps only root, so those files belong to
nobody inside.
