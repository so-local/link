# Troubleshooting

What to do when [the sandbox](README.md) can't start here, a program in it can't
write, run or reach what it needs, or [your agent's grants](../../../../../agents/agent/docs/security/sandbox/permissions/README.md) don't hold. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk sandbox check --ready
```

## It can't start here

Told "the sandbox needs bubblewrap", run `sudo apt install bubblewrap`.
On Ubuntu, when bubblewrap can't make the sandbox, run `lnk sandbox
check --ready`, and let it add the profile.

## It can't write its cache or temp files

`--files.write` a folder, and point `TMPDIR` there, so the program keeps them
in a folder it may change.

## A program is refused

"Permission denied" running a program: add it to `[commands] allow`, or
`--commands.allow`. `gdb`, `strace` or Docker failing on Linux are refused
inside, so run them outside.

## A commit or `git config` fails, read only

`--files.write_dot_files .`, or `[files] write_dot_files`. Told "... is a
link" on Linux, make the dot file a folder or file, or name it with
`--files.write_dot_files`.

## A specification "hasn't run here before" without a terminal

Read it, then add `--yes`, which answers the question a terminal would
have asked.

## It can't resolve names or connect

Add `--internet.hosts '*'`, or the hosts it needs. A program still
offline with `--internet.hosts` ignores `HTTPS_PROXY`, so set its own
proxy setting. git over SSH failing with `--internet.hosts` needs `--internet.tcp
github.com:22` too.

## A program gets `407` from the proxy on a Mac

It drops the user and password `HTTPS_PROXY` names, the run's secret:
give it its proxy setting with them.

## A call hangs, then is refused

It asked you and got no answer: answer it with `lnk sandbox question list`, or
for an agent's sandbox by replying with its code on your chat. A
login that can't open a browser needs `--browser.hosts <host>` (`'*'`
for any).

## Every call is refused

`lnk sandbox log show` shows if it's stopped or paused. Then `lnk sandbox
start` opens every wire, or `lnk sandbox start <name>` one sandbox's.

## Your agent's permissions

What to do when your agent's sandbox refuses a debugger, a start or a
server it should reach, as [Permissions](../../../../../agents/agent/docs/security/sandbox/permissions/README.md) grants them.

### Linux: `gdb` or `strace` fails in the sandbox

Expected, since the sandbox refuses `ptrace`, which debuggers and tracers
need: run them outside the sandbox.

### A start refuses to run with no sandbox

One of three: `~/.lnk/safe/agents/policy.toml` says `unsandboxed = false`; the
machine is a box (root's `/etc/lnk/kind.toml`), whose agents always run
in their sandbox; or `agents.toml`, or a folder on its way, is one
others than you and root could write (or could have removed it from).
Use a harness that runs in the sandbox, or fix the folder's mode.

### It can't reach a server on this computer, a NAS or Home Assistant

For a server here, [`lnk agent permission add --localhost.ports <port>`](../../../../../agents/agent/docs/security/sandbox/permissions/README.md#choose-what-your-agent-can-reach). For a NAS or Home
Assistant, `lnk agent permission add --lan.hosts nas.local`, or `--network.mode
host` for what ignores its proxy.

### Something works only with `--network.mode host`

It ignores `HTTPS_PROXY`, so its calls never reach the proxy: keep that
grant for it, and please report it.

### It answers nothing since `deny --internet.hosts '*'`

Its proxy then reaches only the hosts you allow after it: add its
model's and connections' hosts with `lnk agent permission add --internet.hosts`,
or allow `'*'` again. An allow beside `"*"` changes nothing, since
`"*"` holds every host already.

### `lnk agent permission add network` says `unexpected argument`

`allow` and `deny` take a specification's flags: `--internet.hosts '*'`, `--files.read
'~'`, `--network.mode host`.
