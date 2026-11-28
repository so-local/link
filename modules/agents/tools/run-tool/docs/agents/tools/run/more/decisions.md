# Decisions

Why each `run` call gets a sandbox of its own, which programs it offers,
and how a command runs in the background.

## Why does `run` run each call in a sandbox of its own, and `file` a session's in one?

So code runs only in a sandbox the agent's sandbox file makes. Link
Harness runs no code: it calls `run` and `file` as tools outside its
sandbox, through its proxy, and their calls run under [`lnk sandbox run`](../../../../../../../../security/sandbox/docs/tooling/sandbox/guide.md#sandbox-guide)
with flags made from the file. A program a `run` call leaves running is
gone at the next call, on Linux always and on macOS unless it [left the
call's process group](../guide.md#programs-that-leave-on-macos). `file`
runs only Link File, which starts nothing, so one sandbox with the same
flags serves a session's calls and avoids starting another sandbox for
each read or edit.

## Which programs does `run` offer?

Those of the sandbox file's `[commands]` this computer has, found at a
start using [program discovery](#its-path). The model is told only
those, by name, in the call's description and in its environment
section, so it never tries Python where there is none. The call itself
is offered only where the shell is among them, and a program the shell
starts that isn't among them is refused by the sandbox itself. With
`allow = ["*"]` it is told it may start any program on its `PATH`.

### No asking

There's no asking: `[commands]` allows or refuses. Link's list is a
shell, Python, Node, git, curl, jq and the usual file tools, so a script
works as it would in a terminal. A toolchain (`toolchains = ["rust"]`)
adds a language's programs and what its builds run, and an empty
`[commands]` turns `run` off.

### Any program by default

Link's default sandbox file also has
`allow = ["*"]`, any program on the agent's `PATH`, so the model is
offered a program by name rather than from a list. Drop
`allow` to keep only the toolchains. `run` is granted in Link's default sandbox file on your
own machine, and a deployment's own default may leave it out.

### Its `PATH`

Link first uses the `PATH` captured when the owner last started the
agent, followed by the system program folders. This includes programs
installed in the owner’s home when that captured path names them.
[Program discovery](../../../../../../../agent/docs/agents/more/settings/README.md#where-it-finds-programs)
owns the search order and toolchain access.

## Why does `run` take a command line?

A tool is only as good as the model calling it, so `run` takes the call
models are trained on, Claude Code's Bash: `command`, `timeout` in
milliseconds, `run_in_background` and `description`. A model writes a
shell command line far more reliably than a program and its arguments
split by hand, and it expects pipes, redirections, `cd` and quoting to
work. The shell, `sh -c`, reads it.

### What `[commands]` can check

The tool checks one program, the shell, so `run` is offered only where
`[commands]` grants it. A narrower list can't be checked call by call:
the command line is the shell's to read, and a word in it may be a
program, a builtin or an argument.

### What the sandbox holds

The sandbox holds what the tool
can't. Every program the shell starts is held to `[commands]` by the
kernel, as before, and the files and network a command reaches are the
sandbox file's alike. What runs unchecked by `[commands]` is the shell's
own builtins, which read and write only what the sandbox lets the shell
reach. A list without the shell keeps its other programs for Link Code's
servers and the desktop, and gives `run` no call.

### Its timeout

A call's `timeout` is how long the command runs, and one past the most
(`[tool.run] timeout`, or `background_timeout` in the background) is the
most rather than refused: models ask for the longest their training
allowed, and a refusal would only cost a turn.

### No input, no folder

Bash takes neither, so neither does `run`: a command that needs input
pipes it in (`printf ... | program`), and one that works elsewhere `cd`s
there. Each call starts in the files folder, opened without following a
link.

## What does `run` reach?

What the sandbox file grants the agent: the internet through a proxy of
the call's own when it has `[internet]`, only its `hosts` unless `"*"`,
its `tcp` endpoints, its `per_minute`, and your home folder with
`read = ["~"]`. It reads Home and the agent's instructions and skills,
of its conversations only the calling one's (a stranger's call none),
and writes only the folders `[files] write` names, so it can't rewrite a
conversation. `[localhost]` and `[browser]` aren't
given to it: they're for the harness's own servers and your browser, not
a script's.

### Not the agent's tools

A tool's address is its harness's alone. A program holding it could
call the tool for as long as the agent runs, so `run` is never given the
agent's tools.

## Why is a command stopped before its call times out?

A command stops a tenth before `[tool.run] timeout` (2 s at least, half at most), with everything it
started, so what it printed so far comes back and the harness isn't left
waiting on a call that can only time out. What it printed is kept to
`[tool.run] output` for each of its output and its errors, its start and
its end, which is where a program says what it's doing and how it ended.

## Where is a team's folder for `run` and `file`?

`file` names it `teams/<team>/` in its paths, beside the agent's own
folders, and keeps to what's granted of it. `run` reaches it by its
path, `~/Link/Teams/<team>/shared`, which its sandbox opens as granted.
Link Harness tells its model both.

## Why are `run` and `file` only for Link Harness?

They're defined for a harness that keeps to the files folder's layout,
which writes only its conversations there itself and leaves the rest to
Link's tools. OpenClaw, Hermes Agent and DeepSeek Harness have files, a
shell and code of their own, in the whole folder, so Link adds none. An
agent whose sandbox is off has neither: code runs only in a sandbox.

## Why can a program outlive its call on macOS?

macOS has no way to hold a process tree but its process group, which a
program leaves with `setsid` or a daemon's double fork. Link says so
rather than promising more, and the program stays in the sandbox it
started in. Following every program a call starts, wherever it moves,
needs descendant tracking.

## Background commands

How a command runs in the background, when the agent is told it ended,
and what happens to it when the agent stops.

### How does a command run in the background?

Under a supervisor of its own, `lnk-run-tool supervise`, which `run`
starts and which is up only while its command runs, so an agent with
none costs nothing. `run`'s server ends with its turn, so it can't own a
command that outlives it. The command runs in the sandbox a
`task__run` call gets, in a process group of its own.

### Its lock

The supervisor holds a lock on the command's record for its whole life,
which the system releases however it dies, so a free lock always means
it's gone. They're called background commands, not jobs: each is one of
the agent's `run` calls, on its computer, in its sandbox.

### Calls of `run`

A background command is `task__run`'s `run_in_background`, as it is
Bash's, and three more calls of the `run` tool reach it (list, read and
stop), not a tool of their own: a second tool would repeat the shell,
the sandbox, `[commands]` and the grant, and could disagree with the
first. The flag asks nothing, since `run` asks nothing.
The defaults are opinions, so they're the specification's: 4 at once (a build, a
test run, a server and a download), an hour's timeout (a long test
suite), and output kept a day (a morning's builds read in the evening).

### When is an agent told a background command ended?

When it ends, through the agent's bridge, as a message from `run:<id>`
to the conversation that started it, so it's never taken as yours. It
says how the command ended and its output's end, half of `[tool.run]
output` as a command's end is kept.

### At most once

It's told at most once. The record is marked before the message is
handed over, and unmarked only when the bridge can't be reached, as a
schedule is fired at most once. A message twice is worse than one lost
in a crash, and `task__list` still shows how it ended.

### Down a chain

The message is one further down a chain than the call that started the
command, and a start as far down as `[tool.scheduler] hops` is refused,
so a command started each time one ends stops at the same cap as
messages.

### What happens to a background command when the agent stops?

It ends. A command lives as long as its agent runs, so nothing is ever
left running with no agent to hear of it, and nothing started under
grants since changed runs on. A turn ending, `run`'s server ending or
the bridge restarting leaves it running, and `task__list` finds it again.

### What ends it

Stopping or restarting the agent, its service or the computer ends it,
recorded lost and told when the agent's back.

### The sweep

A sweep, as the bridge starts and at each turn, finds one whose
supervisor died without a word. It kills what's left of its process
group only when it's surely the command's: on the same boot, its leader
the one recorded or gone, since a group's id isn't reused while any of
it runs. A move or a delete stops every one, and none is carried, since
a process can't move.

### Why is a background command's output in the files folder?

So the agent reads it with its own tools, `grep` over a build's log
among them, not only a call at a time. It's `background/<id>.out`, which
the agent's sandboxes read and none writes unless `[files]` says so, and
an ending memory doesn't index, so a build's output doesn't crowd its
search. It's written outside the sandbox through descriptors, never
following a link. The record, which names the process group a sweep may
kill, is kept outside the files folder, in the agent's folder, where
nothing the agent runs can forge it.
