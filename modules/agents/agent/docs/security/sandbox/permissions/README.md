# Permissions

**What your agent may do comes in two layers: what you grant it, and what
its harness asks for beyond Local Link's defaults.** By default your agent
changes only its own folders and reaches the internet through a filtering
proxy, in Local Link's [sandbox](../../../../../../security/sandbox/docs/security/sandbox/README.md).

## Choose What Your Agent Can Reach

What you grant is your agent's own and follows it to any harness.
`lnk agent permission add` and `remove` save the change. A running background agent restarts
with your terminal confirmation or `--yes`; otherwise it uses the saved
change at its next start. Restart a foreground agent from its own terminal.
`lnk agent show --permissions` marks each grant as yours or [its harness's](#allow-what-a-harness-needs).

```sh
lnk agent show --permissions
lnk agent permission add --files.read '~'
lnk agent permission remove --internet.hosts '*'
lnk agent permission add --internet.hosts api.anthropic.com
lnk agent permission add --internet.tcp github.com:22
lnk agent permission add --tools.names gh
lnk agent permission add --internet.per_minute 600
lnk agent permission add --localhost.ports 5432
lnk agent start --sandbox sandbox.toml
```

### Add and Remove Grants

`add` and `remove` take the flags [`lnk sandbox run`](../../../../../../security/sandbox/docs/security/sandbox/guide.md#sandbox-guide) takes, one for each key of a
[sandbox's specification](../../../../../../security/sandbox/docs/config/specs/sandbox.md):
`add` adds what the flags name (`"*"` takes the place of the rest of its
list, which then holds `"*"` alone), and `remove` takes each item away again
(`remove --network.mode host` puts the network back behind its proxy).

### Refuse a Removal That Widens

A removal never widens access, so Local Link refuses one that would and leaves the file as it was: `--internet.per_minute` (a
limit changes only with `add --internet.per_minute`), one item of a list that
holds `"*"` (remove `"*"`, then add each), a folder taken from `hide`,
or one that leaves an OS's table (`[internet.linux]`) without a key it
gave, since that OS would then have the section's own (change that
table in the file itself).

### Know the Defaults

By default, the harness writes only its home, your files folder, and `/tmp/<name>*`,
and reaches the internet only through Local Link's filtering proxy, never this
computer's other servers or your home network. What you grant opens
more.

### Edit the Sandbox File

Local Link keeps what you grant in the agent's sandbox file,
`~/Link/Safe/Agents/main/sandbox.toml` for the agent `main`, a sandbox's
specification, which `lnk agent start --sandbox <file>` reads and
`permission add` writes.

```toml
# ~/Link/Safe/Agents/main/sandbox.toml
schema = 1
[files]
write = ["work", "instructions", "skills"]
read = ["chats", "~"]

[internet]
hosts = ["api.anthropic.com", "api.telegram.org"]
tcp = ["github.com:22"]
per_minute = 600

[localhost]
ports = [5432]

[tools]
names = ["gh"]

[commands]
allow = ["sh", "python3", "git", "/opt/tools/bin/convert"]

[commands.macos]
developer_tools = true

[teams.research]
write = ["drafts"]
```

### What Each Section Grants

`[files]` is what the agent sees: the folders it may write, and those it
only reads, where `"~"` is your whole home, read only. `[internet]` is
reached through its proxy, and `["*"]` is any public host. `[localhost]
ports` are this machine's ports it reaches, such as a service's.
`[commands]` is what its run tool may start, `["*"]` for any.
`[commands.macos] developer_tools` lends Xcode's tools, read only, so
`git` runs. `[teams.research]` is a team's shared folder: what of it the
agent writes, and what it only reads.

### Start Without a File

A section left out stays closed, as in any specification. A missing file grants
no additional files, programs, or network. Local Link recreates the file with closed
permissions and reports the path. A failed recreation does not restore
default grants.

### Know Why a Line Is Refused

Local Link refuses a line, and the error names it, when the line holds an unknown key,
a folder it doesn't see, `unsandboxed` (no sandbox is the agent's own
setting, `lnk agent start --no-sandbox`), another agent's memory,
scheduler, run or file (`memory-<id>`, `scheduler-<id>`, `run-<id>`,
`file-<id>`), a tool named as one of Link's namespaces (`task`, `tool`,
`file`, `agent`), a model's port (its model is its own), or a command
that isn't a program's name or an absolute path.

### A file from an older Link

A sandbox file from an older Link, which named `permissions`, is read
once and written back as these sections: `network` as `[internet] hosts
= ["*"]` (its hosts, `tcp` and `calls` with it), `read-home` as
`[files] read = ["~"]`, `open` as `[browser]`, `developer-tools` as
`[commands.macos] developer_tools = true`, and `lan` with `network` as
`[network] mode = "host"` (with `localhost`, every port).

#### What it can't say as narrowly

That is left out and said once: `localhost` without `lan`, which behind
its proxy reached every port (name each, `--localhost.ports`), and `lan` without
`network`, which opened nothing. Its `ports` without `network`, which on
Linux it didn't reach, it reaches from then on, relayed into its sandbox
as on macOS, and that's said too.

## Each Grant

`lnk agent show --permissions` shows each thing it may do here, as its flag:
the developer tools only on a Mac. A table for another OS is still kept,
so an agent's grants work on both.

### Hosts

`[internet] hosts`, `--internet.hosts`, on by default as `["*"]`: with a
list, its proxy lets it reach only those. Keep its model's host and its
connections' on it, which are `api.telegram.org`, `slack.com` and
`*.slack.com`, `discord.com` and `*.discord.gg`, and `*.signal.org`.
`"*"` is any public host: `remove --internet.hosts '*'`, then `add
--internet.hosts` for each, keeps it to a list.

### Ports beyond HTTPS and HTTP

The proxy passes HTTPS and HTTP. Name anything else as `host:port`
(`--internet.tcp`), such as `github.com:22` for git over SSH with a key in the
harness's `~/.ssh`. On Linux a database client reaches a named endpoint
too, but on a Mac only a client that takes a proxy command, like git
over SSH, does.

### Tools

Add one with [`lnk sandbox tool add`](../../../../../../security/sandbox/docs/security/sandbox/reach.md#call-a-tool-that-runs-outside), then `allow --tools.names`. Any other
harness finds it in its own MCP settings, and Link Harness at
`$LNK_TOOL_<NAME>`. Each is an HTTP MCP server on its proxy, and calls
you didn't allow by name ask you first.

### Calls and Pages

With a limit (`--internet.per_minute`), its proxy takes at most that many calls
a minute and answers the rest with a 429 until the minute has room.
`[browser] hosts` (`--browser.hosts`), off by default, opens a page in
your browser, asking you each time.

### This Machine's Ports

`[localhost] ports` (`--localhost.ports`), none by default, each named: a database
or a service running here. Its harness and its `run` reach them at the
same port, with or without the internet. Every port, `"*"`, only with
`[network] mode = "host"`. Moved to another machine, they're left out.

### Your Network and Your Home

`[lan] hosts` (`--lan.hosts`), off by default, reaches machines of your
network, through its proxy. `[files] read = ["~"]` (`--files.read '~'`), off
by default, reads your whole home folder, and on macOS
`[commands.macos] developer_tools` (`--commands.developer_tools`), on by
default, reads Xcode's tools, so `git` runs.

### Files

Its files folder, its Home, holds `work/` (anything it makes) and
`memory/` (the documents its memory searches); its own folder beside it
(`~/Link/Agents/<agent>/agent`) holds `instructions/` (what it's told to
be), `skills/` and `chats/`, named as Home's folders are.

#### `write` and `read`

`[files] write` names the folders it may change, and `read` those it
may only read; it reads the rest of Home. By default it writes `work`,
`instructions` and `skills`, so asked to change its standing
instructions or a skill, it does. Move `instructions` or `skills` under
`read` to keep them yours alone: it then writes only its notes, through
its memory's `remember`, and `instructions` under `read` locks those too.

#### All of Home

`write = ["."]` is the whole of Home, for a files folder that's a
repository it works in, beside which only `instructions`, `skills` and
`chats` are named. `chats` is written only by its
harness.

#### Each Harness's Files

Link Harness's sandbox reads Home and its instructions and skills, and
writes only those and its conversations. A harness from outside Link
works in the whole of Home and never reaches its own folder, so for it
`[files]` holds only for Link's tools, and a start says so.

### Commands

The programs Link Harness's `run` may start, each call in a sandbox of
its own made from this file. A name is found on the system's `PATH`
(`/usr/local/bin`, `/usr/bin`, `/bin`, and on a Mac
`/opt/homebrew/bin`), and only those this computer has are offered.
Link's are a shell, Python, Node, git, curl, jq and the usual file
tools. A toolchain names a language's programs in one word, with what
its builds run, such as Rust's build scripts:

```toml
schema = 1
[commands]
allow = ["jq"]
toolchains = ["shell", "git", "rust"]
```

[Choose the programs it may run](../../../../../../security/sandbox/docs/security/sandbox/reach.md#toolchains)
explains toolchains, and [`lnk sandbox permission list`](../../../../../../security/sandbox/docs/security/sandbox/security/permission/README.md#list-the-permissions) lists them.

#### Landlock

On Linux a finite list, or a sandbox file without `[commands]`, needs
Landlock. Without that capability, the requested program is refused
before it starts. A grant names its programs, or toolchains that stand
for them, so no list is lifted, but for `allow = ["*"]`, any program,
which Link's template has so builds and what they make run. An allowed shell or Python
interpreter can evaluate code, whose processes inherit the file and
network restrictions and run only the programs the list names.

### The Network as It Is

`[network] mode = "host"` (`--network.mode host`), off by default: for a
NAS, Home Assistant or a model on another computer that `[lan]` through
its proxy doesn't reach. It brings back the known gaps that `lnk agent
harness check` shows as `open`.

#### What Opens Nothing

Nothing then filters or logs its network, so a list of hosts, `tcp`,
tools and pages, [which only its proxy carries](../../../agents/more/security.md#permissions), open nothing, Link's own
tools among them.

### No sandbox at all

No sandbox is no grant: `lnk agent start --no-sandbox` runs your agent
without one, so its harness reads your whole home folder, until `lnk
agent start --sandbox`. Every start warns, and `lnk agent show` shows
it. A harness that can't run in a sandbox says so, and its start is
refused until you add `--no-sandbox`.

#### Keep the Proxy Anyway

The agent keeps its proxy, its tools, the log, and [`lnk sandbox stop`](../../../../../../security/sandbox/docs/security/sandbox/guide.md#stop-a-sandbox) with
it, but the proxy is then [advice the harness may ignore](../../../../../../security/sandbox/proxy/docs/security/sandbox/more/proxy/README.md#with-no-sandbox), not a wall. The sandbox itself,
its proxy, [its log](../../../../../../security/sandbox/docs/security/sandbox/security/log/README.md#see-every-call-on-the-wire) and stopping every agent's way
out at once (`lnk sandbox stop`) are on the [sandbox](../../../../../../security/sandbox/docs/security/sandbox/README.md)
page.

## Allow What a Harness Needs

What a harness asks for beyond your agent's grants is the harness's own,
asked when your agent switches to the harness and held only while it runs.
[Harness Permissions](../../../agents/harnesses/use/harness-permissions/README.md)
says how Local Link asks, how you grant with `lnk agent harness-permission add`, and how you
take a grant back.

## Check the Sandbox

`check` tries, from inside the sandbox, what the harness shouldn't be
able to do, such as reading your home folder or other programs'
secrets, and reports each try.

```sh
lnk agent harness check --harness link
lnk agent harness check --harness hermes
```

### Read the Report

`blocked` when the sandbox stopped it, `allowed` when a grant of yours
let it through, and `open` for a known gap, listed in
[Security](../../../../../../security/sandbox/docs/security/sandbox/security/gaps.md#known-gaps).

### `LEAK`

Something no grant allows got through. The command fails. Please
[report the leak](../../../../../../../docs/more/vulnerabilities.md).

## Run a Command in Its Sandbox

`run` starts the command the way the harness runs, with its home as
`HOME`, in your agent's files folder, with its grants and through its
proxy. So [`lnk sandbox log show`](../../../../../../security/sandbox/docs/security/sandbox/security/log/README.md#see-every-call-on-the-wire) shows its calls, as
`<agent>-<harness>-run`. It runs beside your agent, which keeps running.
Its proxy's calls are your agent's, granted what your agent is granted
at each call, whether or not your agent runs: while it's stopped, `run`
binds its key for its commands, shared by every `run` at once, and the
last to end unbinds it.

```sh
lnk agent harness run --harness link -- curl -sI https://example.com

lnk agent harness run --harness link -- sh

lnk agent harness run --harness hermes -- sh -c 'ls ~'
```

## When Something Goes Wrong

[Troubleshooting](../../../../../../security/sandbox/docs/security/sandbox/troubleshooting.md#your-agents-permissions) covers a debugger that fails, a start that refuses no sandbox, and an agent that can't reach a server, a NAS, or its model.
