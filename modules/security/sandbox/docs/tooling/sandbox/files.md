# Files

The files `lnk sandbox` keeps, and what each one holds. Check this machine’s sandbox support before reviewing its saved configuration.

```sh
lnk sandbox check --ready
```

## Sandbox (`lnk sandbox`)

A program run by `lnk sandbox run` keeps only `PATH`, `HOME`, `TERM`,
`COLORTERM`, `LANG`, `LC_ALL`, `LC_CTYPE`, `TZ`, `USER`, `LOGNAME`,
`SHELL` and `SSL_CERT_FILE` from your shell. [`lnk help <command>`](../../../../../../docs/more/modules/guide.md#get-help) lists the environment
variables each command reads, as [`module.toml`](../../../module.toml)
declares them.

## A proxied policy's files

What `~/.lnk/safe/sandboxes/` (700) holds for each policy: its rules, one
Seatbelt profile per policy, and these.

### `~/.lnk/safe/sandboxes/<name>.routes.json` (600)

A proxied policy's routes (its tools, models, secrets, pages it opens)
and who calls them, which its proxy hands each call's route to its
bridge with. Never for `lnk sandbox run`, which hands its proxy them on
a pipe.

#### Nothing secret

A hosted model's key and the secrets are named, each read by its bridge
from your secrets at each call; one holding a value is refused, written
or read.

### `~/.lnk/safe/sandboxes/<name>.url-secret` (600)

The secret the URLs of a proxied policy's routes carry, which its proxy
requires on each of them.

## Bridges

`<runtime>/lnk/sandbox/bridges/<module>.sock` is a module's bridge for its proxies'
routes (`models`, `secret`, `sandbox`), this user's. Its calls are
proved against the machine's keyring.

### How long it runs

It's started by the first proxy that needs it, and gone once idle, or as
soon as the keyring binds no one (the last agent stopped, the last
sandbox ended), ending any call still open there, a long poll's among
them. A lock beside it keeps it to one. An upgrade that replaces its
program ends it once no call is in flight, and the next call starts the
new build.

### Its own service

On Linux, where systemd runs Link's services, it runs in a user unit of
its own, `lnk-bridge-<module>`, so no agent's service holds it.
Restarting one agent leaves the bridge, and every other agent's tool
sessions, running. Its warnings go to `<module>.log` beside its socket,
kept once as `<module>.log.1` past 1 MiB. Elsewhere it runs in a process
group of its own.

### Known limit

The proxy that starts the bridge reaches systemd's user manager only
when its own environment holds `XDG_RUNTIME_DIR`. A proxy started by an
agent's service is expected to inherit it from that service, but this
hasn't been checked on a box yet. Without it, the bridge starts in a
process group of its own, inside the first agent's service again, and
restarting that agent stops it. `systemctl --user status
'lnk-bridge-*'` shows whether a bridge runs in its own unit.

## The wire

What the proxies log, count and wait on.

### `wire.log` and `asks/<id>.json`

`~/.lnk/safe/sandboxes/wire.log` (600) is `lnk sandbox log show`'s: one JSON object
per proxied call. `~/.lnk/safe/sandboxes/asks/<id>.json` (600) is a question a
proxy is waiting on (`lnk sandbox question list`).

### `~/.lnk/safe/sandboxes/traffic/<pid>.json` (600)

`lnk sandbox show`: the bytes a running proxy has carried each way,
written every 5 seconds while it changes, with when its proxy started.
Read only by `lnk sandbox show`, whose `--json` other modules read.

#### When it goes

Its proxy removes it when its program ends, or `lnk sandbox show`
does once its proxy is gone.

### `~/.lnk/safe/sandboxes/stopped` and `paused/<sandbox>`

While `stopped` exists, every proxy refuses every call. While
`paused/<sandbox>` (600) exists, that sandbox's proxy refuses every
call.

## Tools

The tools that run outside, and the hubs of those shared.

### `~/.lnk/safe/sandboxes/tools/<name>.json` (600)

A tool that runs outside (`lnk sandbox tool add`): its command, or
`mcp-remote` and its URL, and its environment, headers included.

### `<runtime>/lnk/sandbox/tools/<key>.sock` (600), `.lock`, `.log`

A shared tool's hub (`--shared`): its socket while it runs, its lock,
and its log.

#### `<key>`

It names the tool by its name and as it's defined, signed with `tools/.key` (600), a
secret made once on this machine, so the name gives away none of the
tool's tokens. It alone names the socket, without the tool's name, as
macOS keeps a socket's path to 104 bytes and its runtime folder takes
about 60.

## `~/.lnk/safe/sandboxes/settings.toml`

The sandbox module's settings, over Link's. A proxy, the sandbox's
bridge and a shared tool's hub read it from the folder they're handed,
never through the `HOME` they run with, which for an agent is its
harness's. A file that can't be read is said, and Link's are used.

### `ask_wait`

How long a question waits for your answer: `"120s"`, at most `"24h"`;
silence is no. An agent's sandbox waits as its specification's `[connections]
ask_wait` says instead.

### `tool_starts` and `tool_sessions`

`tool_starts` is the tool servers the bridge starts at once for one
sandbox (2), each sandbox's apart, so one starting many never keeps
another's waiting, and `tool_sessions` the sessions one sandbox keeps of each tool
(8): a new one ends that tool's least recently called, never another
tool's, so a sandbox with more tools than that keeps a session of each.

### `tool_idle` and `shared_idle`

`tool_idle` is how long a tool's session stays, its server with it,
with no call (`"5m"`), and `shared_idle` how long a shared tool's server
stays up with no session open (`"10m"`). A sandbox whose caller the
keyring binds no more (it stopped) has its tool sessions ended within a
second, whatever `tool_idle` says: no one is left to call them, and a
tool kept for it, such as its memory session, would keep what it reaches
running, the bridge among them.

## On Linux

What a sandbox keeps of this machine and of its runs, on Linux alone.

### `~/.lnk/safe/sandboxes/placeholders.json` (600)

The empty placeholders made where a project's dot file isn't, while
sandboxes run, the dot files to sweep in Link's data folders when a run
ends, and which runs hold each; removed with the last.

### `~/.lnk/safe/sandboxes/bwrap-version` (600)

What a sandbox's start found of this machine, so the next doesn't ask
again: whether its bubblewrap can refuse user namespaces inside, and
the checks that passed (bubblewrap makes a sandbox here, and `wrap`'s
requirements hold). [`lnk sandbox check --ready`](../../../confine/docs/tooling/sandbox/wall/README.md#make-the-sandbox-ready) removes it and checks afresh.

#### When it's asked again

It holds for this boot, kernel and user, the kernel's user namespace
settings, and these copies of bubblewrap and `lnk-sandbox`: any of
those changed, or a file that isn't a plain one only you may write, and
they're asked again.

## `~/.lnk/safe/sandboxes/seen.toml` (600)

[The sandbox specifications run here](../../config/specs/sandbox.md), each by its path and a hash of what it
opens.

## Command Arguments

The command metadata names the trailing argument list `commands` for
`sandbox run` and `sandbox learn`. It is separate from `--commands.allow`,
which selects programs the sandbox may run. Arguments still follow `--`.
Serialized run and learn inputs use `commands` as their field name.

## Internal Environment Inputs

These inputs belong to process startup and isolated tests. A program inside a
sandbox cannot use them to enlarge the policy that confines it.

### `LNK_SANDBOX_LEARN_FD`

The first sandboxed program receives the descriptor used to record started
programs and removes the variable. A program that sets it inside the sandbox
can record its own descendants only, rather than observe unrelated execution.

### `LNK_TEST_ON_CLOUD`

A value of `1` makes the proxy apply cloud-machine LAN restrictions for tests.
It cannot make a cloud machine count as a non-cloud machine, so it cannot
relax those restrictions.
