# Sandbox Specification Guide

How Link reads a sandbox specification: what each section opens and refuses, how
one OS's table and field flags change it, and what a file may never
grant. Each field is in the [Sandbox Specification](../sandbox.md) reference.

```toml
schema = 1
name = "app"
version = 1

[files]
write = ["~/code/app"]
read = ["~/Documents/reports"]

[internet]
hosts = ["api.openai.com", "github.com"]
tcp = ["github.com:22"]

[commands]
allow = ["sh", "node", "git", "/usr/lib/git-core/"]

[commands.macos]
allow = ["sh", "node", "git", "/Library/Developer/CommandLineTools/usr/libexec/git-core/"]
developer_tools = true
```

## Sections

A section is a grant, and one left out stays closed: a specification with no
sections reads none of your files, reaches no network and runs no
program. A section needs its main key: `[files]` needs `write` or
`read`, `[internet]`, `[lan]` and `[browser]` need `hosts`,
`[localhost]` `ports` or `models`, `[tools]` `names` and `[commands]`
`allow`. An empty section, an unknown key and `"*"` beside other
entries are refused, naming the line, and a specification file is at most 64
KiB. `lnk sandbox permission list` lists each section with its field flags.

## Each Section's Rules

What each section refuses, and how it reaches what it names.

### `[files] hide`

The folders of `write` and `read` inside a hidden folder stay open.
`hide` adds up across the default table, an OS's table and the flags,
where every other key replaces the one before it.

### `[internet] tcp`

Each endpoint names its port; `*.abc.mongodb.net` holds the names under
it, not `abc.mongodb.net`, and `hosts` must allow each. A `*.`
endpoint's port can't be a `[localhost]` port or 40781. On Linux, a specification
with endpoints [gets DNS from its proxy](../../../../proxy/docs/security/sandbox/more/proxy/connect.md#dns-where-a-sandbox-names-endpoints-linux).

### `[lan] hosts`

Which addresses each reaches, and what is refused whatever is listed:
[Security](../../../security/sandbox/security.md#what-can-cross-the-sandbox).
On a cloud's machine, `"*"` and a range holding a whole private block
are refused too, since the network there is the cloud's.

### `[localhost]`

A port is 1 to 65535, and 40781, the port of Link's proxy, is refused in
`ports` and in `models`. A model's port is reached through the proxy,
for inference only, never as a raw port, and `models` alone needs no
`ports`; with `ports` left out, only the `models` are reached.

### `[network] mode`

`"proxy"`, the default, has the program reach the network only through
Link's filtering proxy, as the other sections say. `"host"` gives it
this machine's network as it is, with no proxy and nothing filtered or
logged. A specification with it is shown as reaching the network unfiltered
before it runs.

#### Beside `"host"`

`[internet] hosts` can then only be `["*"]`, and `[lan]`, `[tools]`,
`[browser]`, `tcp`, `per_minute` and `models` are refused, having
nothing to filter them. On macOS, `[localhost] ports` still names the
ports it reaches; on Linux it shares this machine's loopback whole.

### `[tools] names` and `developer_tools`

`[tools] names` are letters, digits, `-` and `_`. `[commands]
developer_tools`, `true`, reads Xcode's and the Command Line Tools'
programs and lets them run, so `git` and compilers run; it means nothing
but on macOS.

### `[commands] allow`

A name is found on `PATH` when the run starts. A path is from the specification's
folder, and one ending in `/` allows every program in that folder.
`lnk-sandbox` and, on Linux, the program loader are always allowed. On macOS, `/bin/sh` is a small program that runs
the shell macOS chose for it, bash unless changed, which
`/private/var/select/sh` names, so allowing `/bin/sh` allows that shell
too. `lnk
sandbox learn` lists the entries a command needs. An empty `allow`, or a
section missing `allow`, is invalid.

#### Installed program links

A name found on `PATH` grants the executable's real path, following
installed links such as Homebrew's `python3`. Link keeps that path for
the launch, so changing the link after discovery cannot redirect the
grant. Explicit paths still refuse links a non-root user owns.

#### What it refuses

An entry in a folder it may write, or in `/tmp`, is refused, and so is a
folder holding one of those (`"/"`, or `~/tools/` beside `write =
["~/tools/out"]`).

#### Landlock

On Linux a finite effective list, after OS overrides, needs Landlock. A
specification without `[commands]` grants no programs and also needs Landlock.
When the required capability is unavailable, the launch is refused with
the observed reason. Every grant names its programs, so every sandbox
on Linux needs Landlock. A command line without a specification keeps its own
defaults.

#### Memory files

With an executable allowlist, Linux seals memory files non-executable
when its kernel and listener support that. Otherwise it refuses
`memfd_create`, so applications using it need a fallback.

## Values

What a path, `"*"` and a host mean in every section.

### Paths

`~/` is your home folder; anything else relative is from the specification's own
folder. `..` is refused.

### `"*"`

In `[internet] hosts`, any public host; in `[lan] hosts`, the private
ranges; in `[browser] hosts`, any host; in `[commands] allow`, any
program; in `[localhost] ports`, every port, only with `[network] mode
= "host"`. It's refused in `[files]` and `[tools]`.

### Hosts

A name with a dot (`api.openai.com`), `*.github.com` for the names under
a name but not the name itself, or an IP address.

## Dot files that run outside

At the top of each folder in `write`, [a project's dot files that run
outside the sandbox](../../../../confine/docs/security/sandbox/wall/README.md#what-every-sandbox-keeps-closed) are read only.
`write_dot_files` lists the folders of `write` where they're written
too, for [a sandbox that commits](../../../security/sandbox/guide.md#let-it-commit).
A folder in it that isn't in `write` is refused. Left out, none is.

## One OS's Table

`[<section>.macos]` and `[<section>.linux]` hold the section's keys for
one OS. Each key given replaces the section's there, but `hide`, which
adds up. A section with only another OS's table is closed here. Its
field flags are its key paths, such as `--files.macos.write`.
`lnk sandbox spec show` prints the specification on this OS, and names the
file's tables for the other, which it leaves out.

## Field Flags and a File

`lnk sandbox run`, `check`, `learn` and `spec show` take the file as
`--spec FILE` and each key as a field flag, its key path:
`--files.write`, `--internet.hosts`, `--commands.allow`. A flag giving a
key replaces the file's key, but `--files.hide`, which adds to it.
Without `--spec`, the flags are the whole specification. Omitted grants stay
closed in both inputs, and a grant always names its programs or toolchains. [`lnk agent permission add`](../../../../../../agents/agent/docs/security/sandbox/permissions/README.md#choose-what-your-agent-can-reach) and `deny`
take the same flags, and Link shows each grant in them.

### Write a File From Flags

`lnk sandbox spec show` prints the specification its flags make, which
you save as a file, such as `app.toml`, to give to `--spec`.

```sh
lnk sandbox run --commands.toolchains shell --files.write . --internet.hosts '*' -- bash
lnk sandbox spec show --files.write ~/code/app --commands.allow node
```

## Teams

`[teams.<team>]` names the folders of a team's shared folder an agent
writes and only reads, each a folder's name in it or `.` for all of it.
Only an agent's harness knows where a team's shared folder is, so this
table is an agent's grant, in its sandbox file: `lnk sandbox run`
refuses a specification holding it, and an agent's `lnk agent permission add
--teams.research.write drafts` grants one folder as any other grant.

## Refused in a file

A specification from a file may be anyone's, so these are refused in it, with
the flags given beside it:

- A folder in Link's settings or agents' folders, or a write of one
  holding them.
- A write of your home folder itself, a dot folder in it (`~/.ssh`,
  `~/.config`), `~/bin`, `~/Library`, a folder on your `PATH` or one
  holding it, or in the [system's folders](#the-systems-folders).
- A write of the folder holding the specification, which would let the program
  change its next run.
- A write in a project's `.git`, `.envrc`, `.vscode` or `.idea`: write
  the project's folder, and name it in `write_dot_files`.
- A read or write in, or holding, a folder where programs keep their
  sockets: `/tmp`, `/run`, `/var/run`, `/var/folders`,
  `$XDG_RUNTIME_DIR` and the temp folder, so `/` too. What answers on a
  socket there, a session's bus or Docker's, runs what it's asked
  outside the sandbox, and on Linux a socket connects read only.
- `..` in a path.

### The system's folders

`/bin`, `/sbin`, `/usr`, `/etc`, `/lib`, `/lib32`, `/lib64`, `/boot`,
`/dev`, `/proc`, `/sys`, `/opt`, `/nix`, `/snap`, `/var`,
`/home/linuxbrew`, and macOS's `/System`, `/Library`, `/Applications`,
`/private/etc` and `/private/var`. Homebrew's, `/opt/homebrew` and
`/usr/local`, are among them.

### Flags, and a run's own folders

Flags alone are yours, or Link's own, and aren't checked this way.
The folders a run is given to write beside its specification's (`lnk sandbox
run --with-fd`'s `write`) are: each must be a folder there, named in
full with no `.` or `..`, and is refused as a write in a file is. A
program `[commands]` lists in a given folder is refused too.

<!-- spec-reference: sandbox -->
