# Sandbox Guide

Run any program so it changes only the folders you give it, reaches only
what you allow, and never reads Link's settings. Link agents use it by
default, and you can use it for your own commands.

```sh
lnk sandbox check --ready

lnk sandbox run --commands.toolchains shell --files.write . -- bash

lnk sandbox run --commands.toolchains shell --internet.hosts '*' -- curl -sI https://example.com

lnk sandbox run --spec app.toml -- node app.js
lnk sandbox check --files.write .
lnk sandbox log show
```

## What it needs

You need a Mac, whose Seatbelt is built in, or Linux with bubblewrap. The
sandbox module comes with every harness, and [`lnk mod add sandbox`](../../../../../../docs/more/modules/guide.md#add-or-remove-a-module)
installs it alone. It's free and needs no account.

## Run a program

How to run a program in the sandbox, with only the folders you give it,
or with all of it written down in a file.

### Run a program in the sandbox

The program reads the system's programs and libraries, and nothing else
unless you open it. It has no network. Its environment is cleared, so no
tokens or `SSH_AUTH_SOCK` from your shell reach it. `run` exits with the
command's status.

```sh
lnk sandbox run --commands.toolchains node --files.write ~/Projects/site -- npm run build
lnk sandbox run --commands.toolchains python --files.read ~/Documents/reports --files.write . -- python3 summarize.py
lnk sandbox run --commands.allow ./tool --files.read '~' --localhost.ports 5432 -- ./tool
```

#### Its arguments

Arguments after `--` reach the program unchanged, including option names
also used by the sandbox's internal launchers.

#### `--files.write` and `--files.read`

`--files.write <dir>` is a folder it may read and write, and `--files.read <dir>`
one it may only read. Both repeat, and `--files.read '~'` reads your whole
home folder except Link's settings. `--localhost.ports <port>` lets it connect to a
port on this machine's loopback.

#### Folders that aren't there yet

`lnk sandbox run` makes a folder it writes that isn't there yet, as it
starts, so a first step that fills `~/app` keeps what it writes. A grant
can't reach a folder that isn't there.

#### `--files.hide`

`--files.hide <dir>` closes a folder to it even then, except for the `--files.write`
and `--files.read` folders inside it. Agents' folders in `~/Link/Agents`, and
what agents that left kept in `~/Link/Archive`, are always hidden this
way.

#### Behind a proxy that re-signs TLS

A company's network often re-signs every TLS connection with its own
certificate authority, which programs trust through `SSL_CERT_FILE`.
The sandbox keeps that variable and lets the program read [the one file
it names](security.md#its-ca-file), wherever it is, read only. It never opens the file's folder
or `SSL_CERT_DIR`'s, which may hold more than certificates.

#### Every flag is a key

Every permission flag is a key of a specification, the file the next section writes. `lnk
sandbox permission list` [lists each section and its flags](security/permission/README.md), and `lnk sandbox run --help`
describes them.

### Describe a sandbox in a file

A specification is a TOML file whose sections are the sandbox's only grants:
`[files]`, `[internet]`, `[lan]`, `[localhost]`, `[tools]`, `[browser]`
and `[commands]`. A section left out stays closed, so a specification with no
sections reads none of your files, reaches no network and runs no
program. [The specification](../../config/specs/sandbox.md)
lists every key and what it takes.
`lnk sandbox spec show` prints the specification its flags make, so it
writes a first file for you.

```sh
lnk sandbox spec show --files.write ~/code/app --internet.hosts '*' --commands.allow node
```

Save what it prints as `app.toml`, beside the program it runs. It
writes `~/code/app` alone, reaches any public host, and runs only
`node`.

```toml
# app.toml
schema = 1

[files]
write = ["~/code/app"]

[internet]
hosts = ["*"]

[commands]
allow = ["node"]
```

```sh
lnk sandbox run --spec app.toml -- node app.js

lnk sandbox run --spec app.toml --yes -- node app.js
lnk sandbox check --spec app.toml
lnk sandbox spec show --spec app.toml
```

```toml
schema = 1
name = "app"
version = 1

[files]
write = ["~/code/app"]

[internet]
hosts = ["registry.npmjs.org", "api.openai.com"]

[commands]
allow = ["sh", "node", "npm"]
```

#### Asked first

A specification can come from anyone, so one you haven't run here, or one that
opens something different now, shows what it opens and asks first.
`--yes` answers for you. A flag beside `--spec` replaces that key of the
file's, except `--files.hide`, which adds to it.

#### What it can't open

A specification can't open Link's settings, agents' folders, or [anything that
would let a program reach outside the sandbox](../../config/specs/guides/sandbox.md#refused-in-a-file).

### Let it commit

At the top of each folder it writes, [a project's dot files that run
outside the sandbox](../../../confine/docs/security/sandbox/wall/README.md#what-every-sandbox-keeps-closed), such as `.git`, `.envrc`, `.vscode` and `.idea`,
are read only.
So `git status`, `diff` and `log` work, and a commit, `git config` or a
new hook needs `--files.write_dot_files` to name the folder, or `[files]
write_dot_files` in a specification.

```sh
lnk sandbox run --commands.toolchains git --files.write . -- git status

lnk sandbox run --commands.toolchains git --files.write . --files.write_dot_files . -- git commit -qam fix
```

#### Why they're read only

What's written there would run as you, outside the sandbox, the next
time you run git, `cd` into the folder or open it in your editor.

#### Only the top

Only the top of each folder is kept read only, so a repository the
program clones or makes in a folder inside is its own to write. Give it
the project's folder, not the one holding all your projects.

## Choose what it can reach

The programs it runs, the hosts, machines and models it reaches, the
tools it calls and the pages it opens, each a flag of `lnk sandbox run`
and a key of a specification: [Choose What the Sandbox Can Reach](reach.md).

## Answer what a sandbox asks

A call that asks you first, such as a tool's or a page to open, [waits
for a yes or no](tools/questions/README.md): `lnk sandbox question list` shows what waits and `lnk
sandbox answer` answers it.

## Check it, and watch it

Try the sandbox from inside, see every call on its wire, and what each
sandbox sent.

### Check the sandbox

`check` plants bait, such as secrets in your home folder, in Link's
settings and in each `--files.hide` folder, and a server on another port. It
tries to reach each from inside, then removes it, and reports each try.
Like every sandbox, it first makes the Link folder and [the folders every
sandbox hides there](security.md#hidden-folders-made-first), where they aren't yet; those stay.

```sh
lnk sandbox check
lnk sandbox check --files.write . --internet.hosts '*'
lnk sandbox check --spec app.toml
lnk agent harness check --harness link
```

#### What it reports

`blocked` when the sandbox stopped it, `allowed` when a folder or
permission you gave lets it through, `failed` when it was allowed but
didn't work here, `missed` when it should have been allowed and wasn't,
and `open` for a known gap.

#### `leaked`

Something nothing allows got through. It exits 1, and please [report it as a vulnerability](../../../../../../docs/more/vulnerabilities.md).

### See every call on the wire

[`lnk sandbox log show`](security/log/README.md) lists each call on a sandbox's proxy, allowed or
refused, without its path, headers or body.

### See what each sandbox sent

This is what each running sandbox's proxy has carried since it started,
up from the sandbox and down to it. [`lnk measure`](../../../../../tooling/measure/docs/tooling/measure/README.md#measure) reads it as an agent's
network; a hosted model's tokens are counted by its route, read from the
usage each answer gives as it passes, and `lnk measure` reads them as
the agent's.

```sh
lnk sandbox show
lnk sandbox show --json
```

## Stop a sandbox

Closing a wire refuses every new call and ends open ones within a
second. `lnk sandbox stop` closes every wire and stops the programs
sandboxed here, however an agent behaves, and the log stays. `lnk sandbox
start` opens the wires again.

```sh
lnk sandbox stop
lnk sandbox start
```

Given a sandbox's name, both act on that sandbox alone: `stop` closes
its wire and leaves its program running, and `start` opens it again.
`lnk sandbox list` names every sandbox that runs or has run here, and
`--paused` keeps those whose wire is closed, with why.

```sh
lnk sandbox list
lnk sandbox stop main-link
lnk sandbox list --paused
lnk sandbox start main-link
```

### Without a wire

A sandbox without a wire, such as an agent with `--network.mode host`,
stops only when its program does. An agent with no sandbox keeps its
wire, but it closes only the calls the agent sends its proxy, which no
wall keeps it to.

### A wire that pauses itself

A proxy also pauses its own wire when its calls look like no normal
work, and notifies you. That means 30 calls refused within a minute, 20
different targets refused within 10 minutes, or any call to a cloud's
metadata service.

#### Seeing it

`lnk sandbox list --paused` and `lnk sandbox log show` show which wires are paused
and why. [`lnk agent show`](../../../../../agents/agent/docs/agents/models/agent/README.md#set-its-model-and-how-hard-it-thinks) shows its own, for a box where no
notification shows.

## Let someone SSH into a sandbox

[Box Users](../../../../../compute/box/docs/services/boxes/user/README.md)
gives each person a Linux login and sandbox on a box. Shells, remote commands,
and file transfers use that sandbox, whose command policy must allow
`/bin/bash`. [Sandboxed SSH Sessions](more/sessions.md) documents the forced
command interface used by a box controller.

## Make the sandbox work on this machine

On Ubuntu 23.10 and newer, `lnk sandbox check --ready` also offers to add
an AppArmor profile with sudo, or prints the two commands. [`lnk agent start`](../../../../../agents/agent/docs/agents/README.md#create-an-agent),
`run` and `check` make sure of this first, once a boot on Linux, and on
a Mac there's nothing to do. On Linux, install bubblewrap first with
`sudo apt install bubblewrap`, then check.

```sh
lnk sandbox check --ready
```

Where sudo asks no password, as on a box, `lnk sandbox check --ready --yes`
installs bubblewrap itself with apt (Ubuntu's package manager). A new
box's package mirror can be slow or end a download midway, so that
install tells apt to fetch a failed download again up to 3 times and to
drop a connection that sends nothing for 60 seconds, then retry it.
The install sets apt's own retry and timeout options to [the same values a box keeps](../../../../../compute/box/docs/services/boxes/lifecycle/on-a-box.md#retrying-apt)
for every apt run on it.

## Where things live

Link keeps the sandbox's files in your settings folder, where no
sandbox reaches them: `~/.lnk/safe/sandboxes/` holds your tools, the wire's log
and waiting questions, and `~/.lnk/safe/sandboxes/seen.toml` lists the specifications
run here, so [a new one asks first](security.md#what-a-specification-can-and-cant-open).

## More about it

What the sandbox guarantees, and its known gaps, are in
[Security](security.md). Why it works this way is in
[Decisions](decisions.md). How a module uses it is in [Module
Contracts](../../../../../../docs/more/modules/contracts.md). What
to do when it fails is on [Troubleshooting](troubleshooting.md).
