# Sandbox Contract

> Define Inputs Shared by Sandbox Callers

`link-sandbox-contract`: the sandbox's policy, independent of its
implementation. It reads a sandbox's specification, resolves its permissions,
and checks addresses and names on the network, with no parser,
backend or proxy, for every side that edits or enforces a policy.

## Each command's arguments

Each [`lnk sandbox`](../../../../../docs/tooling/sandbox/README.md#sandbox) command's runtime arguments are a type here,
generated from `actions.toml`, with `argv()` writing the line that
gives them and `from_argv()` reading one back. The program parses its
command line into the same types, and the checks `actions.toml` names
(`checks.rs`) run on them. So the sandbox building its own proxy's line
(`SandboxProxyArgs`), and reading that line back to hand it its
secrets, uses the declaration, never a flag typed out.

## The specification

`Spec` is [a sandbox's specification](../../../../../docs/config/specs/sandbox.md), a TOML file whose sections are its only
grants:
`Files`, `Network` (its `NetworkMode`), `Internet`, `Lan`, `Localhost`
(its `Port`), `Tools`, `Browser` and `Commands`, each with an `Os`
table. It's here, in its own contract, since more than the sandbox
module keeps one: an agent's file, a harness's grants.

### What it leaves to others

What a specification opens on a machine is the confinement resolver's, and its
refusals that need the machine itself are the sandbox module's: another
module has a specification checked by running [`lnk sandbox`](../../../../../docs/tooling/sandbox/README.md#sandbox).

### Its parts

`spec::grants` is the grant algebra policy editors share (`union`),
`spec::resolve` the OS overrides and authored paths, and
`spec::schema` the schema and documentation made from the type
(`Section`), which [`lnk sandbox permission list`](../../../../../docs/tooling/sandbox/security/permission/README.md#list-the-permissions)
lists and [`lnk spec show sandbox --json`](../../../../../../../tooling/spec/docs/config/specs/guides/kinds/README.md#show-a-kind) prints.

## Addresses and names on the network

`net` holds the address and name rules shared by the sandbox proxy and
other network callers. `forbidden` and `this_machine` identify addresses
refused by public-internet policy: private and loopback ranges, cloud
metadata and this machine's own addresses. Explicit LAN and localhost
grants use their own restricted connection paths; host-network mode
has separate limits. `check_host`, `check_lan` and `check_endpoint`
validate their respective name patterns. `Upstream` selects direct
connections, SOCKS or a local HTTP proxy.

## The programs a sandbox may run

`Programs` is `Only` these, by path: a file, or a folder of them; none,
nothing. A specification always names them, toolchains expanded into their
programs and build folders. `Any` is Link's own, for [`lnk sandbox learn`](../../../../../confine/docs/tooling/sandbox/wall/README.md#naming-a-refused-program)
and `check`'s probes, never a specification's. Its
`requirement_policy` names the effective command policy, `unrestricted`
or `restricted`.

## Folders every sandbox hides

`hidden_dirs` gives the folders every sandbox hides, whatever it's
allowed, but for what it names in them: `~/Link/Agents` (`agents_dir`,
a folder per agent, hidden from every sandbox but an agent's own),
`~/Link/Teams` (`teams_dir`, hidden but for what a member is granted of
it) and `~/Link/Archive` (`archive_dir`, what Link keeps a while of
what's gone).

### A folder no sandbox may write

`unsafe_files` says why no sandbox may write a folder, when it mustn't:
where writing is a way out of a sandbox (programs and settings that run
or are read outside it) or holds the user's keys.

## Variables a caller can't give

`check_given_variable` refuses a variable a caller can't give a
sandboxed program: those [`lnk sandbox run`](../../../../../docs/tooling/sandbox/guide.md#sandbox-guide) keeps or sets itself, the
dynamic loader's (`LD_*`, `DYLD_*`) and a proxy's. Startup hooks and
interpreter search paths are refused too: an environment grant must not
install code before the requested command starts.

## What Linux shows of the machine

`system::LINUX_READS` is what Linux's sandbox shows a program of the
machine outside its own folders, read only, those that exist: the
system's programs and libraries, and selected public settings from
`/etc`. Anything kept in them, whatever its mode, a sandboxed program
running as its owner can read; `/run`, `/tmp` and the homes it gives
fresh.

## A box's users

`box_users` is what the box module writes of a box's users (`lnk box
user`) and every other side reads: where root keeps them, the line that
lets a key in, and the one way into a user's sandbox, for an SSH session
and a run alike.

### Where root keeps them

In `/etc/lnk/users/<name>/`, where neither they nor anything they run
writes: `user.toml` (`BoxUser`, their folder and key), `<name>.toml`
(their sandbox's specification, its paths absolute) and `authorized_keys`, which
sshd reads for them in place of their own, for the group `lnk-users`
(`GROUP`).

### The line that lets a key in

`line` gives it, `restrict,pty,command="..."` entering the sandbox:
`restrict` turns off port, agent and X11 forwarding and `~/.ssh/rc`,
each a way around the sandbox, and `pty` gives back a terminal. Their
login shell is `/bin/sh`, which reads no file of theirs before the
command, and the programs are root's copies in `/usr/local/lib/lnk`.

### Tests

`LNK_TEST_BOX_ROOT` (`TEST_ROOT`) puts a test's folder in place of `/`,
for tests of a stand-in box; only a debug build reads it.

## An ordinary box's workers

`box_workers` is the root launcher of an ordinary box: its settings in
`/etc/lnk/workers.json` (`CONFIG`), `ordinary` with its `Controller`
(`uid`, `gid`, `groups`) or `packed`, and the program it runs,
`/usr/local/lib/lnk/lnk-sandbox` (`PROGRAM`). Packed boxes use their
slot users; computers keep the person's explicit keeper fallback.

### `privileged`

The root-owned helper: only an already authorized controller can ask
sudo, and no sudo rule is given to a worker. It keeps explicitly
inherited descriptors (the proxy's namespace handshake), and the child's
given environment.

## Client and Route Interfaces

[Client Interface](../../../../client/docs/tooling/sandbox/more/contract/client.md) explains how callers ask for confinement while retaining
ownership of the program and environment they run.
[Routes](../../../../route/docs/tooling/sandbox/more/contract/routes.md) defines proxy mounts and bridge calls for models, secrets, and outside tools.

### Model and Secret Routing

[Models and Secrets](../../../../route/docs/tooling/sandbox/more/contract/models-and-secrets.md)
explains placeholder replacement, credential boundaries, and how each service
bridge handles an allowed call without exposing the stored credential.
