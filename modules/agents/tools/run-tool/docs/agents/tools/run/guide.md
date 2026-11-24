# Run Guide

Enable Run to execute shell commands in your agent’s files folder. Choose the programs, writable folders, and network access in its sandbox file before running a command.

```sh
lnk agent start
lnk agent tool add run
lnk agent show --instructions
```

## Ask it to run something

Ask it "what's 2 to the power of 100, in Python?" and it runs
`python3 -c 'print(2**100)'` as a shell command, in its files folder
and in a sandbox of its own.

### What it needs

You need the agents modules, which [`lnk up`](../../../../../../../../docs/more/modules/install.md#pick-your-modules) installs and which
bring Link Run, and Link Harness. A new agent has `run` off:
[`lnk agent tool add run`](../../../../../../agent/docs/agents/README.md#add-tools-and-skills) installs Link Run if it's missing, then turns it on
and restarts the agent, and each start then gives the agent its `run`.
Local program execution has no Link API fee. Your agent’s model calls
and any services the program uses retain their usual cost.

### The toolchains it asks for

When its sandbox file allows no program yet, it asks which toolchains
it may start (a shell and everyday programs, scripting with git, Python
and Node, or all of Link's usual ones), and names their programs before
you approve them. `--yes` takes the shell's.

## Choose what it may run

Your agent may run the programs its sandbox file's `[commands]` names,
of those your computer has. Link's are a shell, Python, Node, git, curl,
jq and the usual file tools, and the language servers [Link
Code](../../../../../code-tool/docs/agents/tools/code/README.md) starts with the builds
they run (`cargo`, `rustc`, `go`).

```toml
# ~/Link/Safe/Agents/main/sandbox.toml
schema = 1
[commands]
allow = ["sh", "/opt/tools/bin/convert"]
toolchains = ["python", "git"]
```

```sh
lnk agent start --sandbox sandbox.toml
```

### The shell runs every command

Your agent's call is a shell command, as the shells models are trained
on take it, so Run offers it only when `[commands]` grants the shell:
the `shell` toolchain, `sh` itself, or `allow = ["*"]`. Without the
shell, your agent has no `run` call.

### Programs the shell starts

Run checks only that the shell may run. Everything in the command line
runs inside the shell, so a narrower `[commands]` can't be checked call
by call. The sandbox still holds it: each program the shell starts, and
each one those start, must be in `[commands]`, or the kernel refuses to
run it. The shell's builtins (`cd`, `echo`, a redirection into `work/`)
are part of the shell, so they run wherever the shell does, within the
files and network the sandbox grants. [Agents
Security](../../../../../../agent/docs/agents/tools/use/security.md#run-and-file)
has the whole of what holds.

### Found on the `PATH`

A name is found on the owner's captured `PATH`, then the system's, and a program your computer
doesn't have isn't offered, so without Python your agent is never
told it has Python. There's no asking: `[commands]` allows or refuses.
[Permissions](../../../../../../agent/docs/security/sandbox/permissions/README.md)
has the whole file.

## How your agent calls it

In Link Harness, [`/task:run`](../../../../../scheduler-tool/docs/agents/tools/scheduler/task/README.md#run-commands) invokes this action with the command’s JSON
arguments and the current agent’s Run grants.

### `task__run`

Your agent runs commands through `task__run`, which takes the arguments
of the Bash call models are trained on: `command`, the command line the
shell runs (`sh -c`); `timeout`, in milliseconds, at most the longest
a command runs; `run_in_background`; and `description`, what it does
in a few words, which the call's title shows. Each call starts in the
files folder, and a command runs elsewhere by a `cd` of its own. A
command reads nothing on its input. The answer is the exit status and
what the command printed.

### Its own sandbox

Each call runs in a sandbox of its own, made from the agent's sandbox
file, which ends with the call, and the next call starts afresh. On
Linux, nothing it starts outlives the call, since the sandbox ends with
everything in it.

### Programs that leave on macOS

On macOS, a call ends by killing its process group, the set of programs
it started that share its group ID. A program that leaves that group, as
`setsid` or a daemon's double fork does, isn't killed with the call and
can run on after it, still confined by the sandbox it started in. So
the run tool doesn't follow every program a call starts, and an agent on
a Mac can leave a program running past its call.

### What it reads and writes

It reads the files folder, its Home, and its instructions and skills in
its own folder (`~/Link/Agents/<agent>/agent`), and writes only the
folders its `[files]` lets it write, `work/` among them, never `chats/`.
Of `chats/`, a call reads only its own conversation's folder, and a
stranger's call none: another conversation's is hidden from it.

### What it reaches

It reaches what the file grants: the internet through a proxy of its
own with `[internet]`, and only the `hosts` it names unless `"*"`. The
agent's tools are [its harness's alone](more/decisions.md#not-the-agents-tools), never a program's. With the agent's route (`lnk agent route`, the exit here), its
proxy goes out through it, as the agent's does, and the network as it
is and the LAN reach nothing past it.

### What it printed, marked

What a command printed, each of its output and errors, comes inside a fence, an opening `<untrusted ...>` line and a closing line with the same random ID, which tells your agent's model that it's data to read and never your instructions, since anyone may have written it. So does a background command's output. [Untrusted Content](../../../../../../../../packages/untrusted/docs/more/libraries/runtime/untrusted.md) explains the fence.

## Run a command in the background

A build, a test suite or a download runs on after the turn that started
it, and your agent is told in the same conversation when it ends.

### Ask for one

Ask it to "run the tests in the background and tell me how they went".
It calls `task__run` with `run_in_background`, which answers at once with
the command's id, and goes on with its turn. When the command ends, a message from it, never
from you, tells the agent how it ended and the end of its output, and
the agent answers in your chat. Background commands are offered to a harness that takes messages from others, which Link Harness does.

### Start a Background Command

`run_in_background = true` starts the command as any `task__run` call
would, the same shell, sandbox and folder, and answers with its id at
once. Its `timeout`, where it gives one, is at most the background
commands' longest, which it is when left out. In Link Harness,
[`/task:run`](../../../../../scheduler-tool/docs/agents/tools/scheduler/task/README.md#run-commands)
takes the same JSON, `run_in_background` among it. The flag asks
nothing, as `task__run` asks nothing: `[commands]` allows or refuses.
It is offered only where the agent has background commands.

### List, Read, or Stop Background Commands

`task__list` gives each command this conversation started, running or
ended: its state, how long it ran, its output's size. `task__read` gives
its output from a byte you give, or its end, and `task__stop` stops it,
with everything it started on Linux and [its process group on macOS](#programs-that-leave-on-macos).

### Only its own conversation's

For `task__list`, `task__read`, and `task__stop`, `kind = "run"` selects background commands rather than scheduler jobs. Each conversation reaches only the commands it started: another's id
is answered as no command of this conversation's.

### Its sandbox and output

A background command runs in the same sandbox a `task__run` call
gets, and its output, both streams as one, is `background/<id>.out` in
the files folder, which the agent reads and can write only if its `[files]` grants
explicitly allow that folder. Its ports are
its own sandbox's, so start a server and its tests in one command
(`server & tests`).

### How long it lives

It lives as long as your agent runs: stopping or restarting the agent,
or the computer, ends it, and the agent is told it was lost when it's
back. Moving or deleting the agent stops every one, and none moves with
it.

## Its limits and settings

A command runs for at most two minutes, or less where its call's
`timeout` says, and 16 KB are kept of each of its output and its
errors, their start and their end. A `timeout` past the most is the
most, so a model asking for ten minutes gets two. Both are keys of
your agent's specification under `[tool.run]`, `timeout` and `output`, and
[Agent Specification](../../../../../../agent/docs/config/specs/agent.md) lists
them all.

### What's kept of its output

Each stream keeps at most half of what Link Harness keeps of a tool's
answer, `[tools] answer` (32 KB), less room for the fences around it, so
the end of its errors always reaches your agent. At the defaults, that's
a little under 15 KB each. Neither stream keeps more than 1 MiB, whatever
`output` says, so an answer always fits the tools bridge's message.
Each session runs at most 16 calls at once, and one past that is refused
until another ends.

### Its background commands

Four background commands run at once, each for at most an hour, and
at most 100 start in any rolling 24 hours, since each one's ending starts a turn. The
newest output is kept up to 16 MB, dropping its oldest half when full, and an ended one's record and
output become eligible for removal a day after it ended, once the agent
has been told. The keys are `background`
(`0` turns them off), `background_timeout`, `background_output`,
`background_keep` and `background_per_day`.

## More about it

[Agents Security](../../../../../../agent/docs/agents/tools/use/security.md#run-and-file)
has the guarantees and gaps, and
[Decisions](more/decisions.md)
why it works this way. To work on its code, see [Developing Link
Run](more/developing.md), and when it can't run a program or a command stops,
[Troubleshooting](troubleshooting.md).
