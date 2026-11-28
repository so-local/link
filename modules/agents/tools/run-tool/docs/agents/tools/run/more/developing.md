# Developing

`lnk-run-tool` is Link Run: an MCP server over stdio that runs an
agent's shell commands, each call in a sandbox of its own. How to use it:
[Link Run](../README.md).

## Build and test

From the repository root, run `cargo build -p link-run-tool`, then `cargo test -p link-run-tool`.

## How it's put together

`serve --files <folder> --programs-path <PATH> --timeout <duration>
--output-bytes <bytes> --program <program>... -- <sandbox>...` is the tool, and Link
gives it everything at a start.

### What Link gives it

The programs of the sandbox file's `[commands]` this machine has
(`--program`), the shell among them or no `command` is offered
(`[tools] conditions`' `shell`, and `background` only beside it), the
specification's `[tool.run]`, and the sandbox, [`lnk sandbox run`](../../../../../../../../security/sandbox/docs/tooling/sandbox/guide.md#sandbox-guide) with the flags
the sandbox file makes, `--commands.developer_tools` among them on a Mac
when `[commands.macos] developer_tools` grants Xcode's tools. No number or grant is the program's own, and
with no sandbox after `--` it refuses to start.

### A call

A call runs `<sandbox> -- <shell> -c <command>`, the shell being the
contract's `limits.toml` `shell` (`sh`) as `--program` names it.

### Variables

With
`--var NAME=VALUE` given, it runs `<sandbox> --with-fd 4 -- <shell> -c
<command>`, and the
variables reach the sandbox on descriptor 4 as the `env` of what
`lnk sandbox run --with-fd` reads, since the sandbox clears the
environment. The sandbox refuses those it refuses any caller, such as
the dynamic loader's `LD_PRELOAD` or a proxy's, and `serve` refuses them
at its start too. No program of their own sets them, so a call runs
only the programs `[commands]` names. Link gives an agent with a
[desktop](../../../../../../desktop-tool/docs/agents/tools/desktop/README.md)
its `DISPLAY` and `XAUTHORITY` so.

### Its own conversation's folder

Link's sandbox flags hide the agent's `chats/`, every conversation's
folder. With `--conversations <chats folder>`, a call whose `_meta` names
its conversation (`link.local/thread`) and an owner's or member's
standing (`link.local/standing`) adds `--files.read` of that
conversation's folder alone to its sandbox. A stranger's call, or one
that names no conversation, reads none.

### Granted Secrets

The agent gives the Run server a protected `LNK_RUN_CONTEXT` through
the tool definition's secret channel. It names the agent, its owner,
the binding lookup, and named sealed routes; it contains no secret values.
Before each command, Run resolves the current binding and hands the
validated routes and caller identity to the sandbox on descriptor 4.
The sandbox consumes and closes that descriptor before starting the task.
The task receives only the route variables needed to reach the proxy.
The proxy checks the current grant when it dispatches a request, as
[Secrets](../../../../../../../../security/secret/docs/config/secrets/README.md#seal-a-secret-for-one-host)
describes.

### Background Context

The same context reaches a background supervisor on a private pipe,
then the sandbox on descriptor 4. Its launch gate uses descriptor 3.
Neither the background record nor a tool cache saves the binding or
the secret value. A task variable cannot replace the context or a
sealed route variable; an invalid or foreign binding fails before
the sandbox starts.

## How a call runs

A call's shell, its folder, its leader and its output, each in turn.

### Its shell and process group

The shell is found on `--programs-path` and runs for the call's
`timeout` (milliseconds), at most `--timeout` and that when left out,
in a process group of its
own. It and its group get `SIGKILL` when `serve` ends, however it
ends, on every system: a watcher forked in its place before its program
runs (`link_secretive::process::watched_by_parent`) waits on `serve`
itself, never on the thread that started the call. At its timeout, or when its caller cancels the
call (`notifications/cancelled`), the group is killed, and once the
program exits, what it left running is too, a second later.

### Its folder, by descriptor

The files folder is opened without following a link, and the shell
starts there by that descriptor (`fchdir`), so a folder swapped for a
link after the check is never followed. A background supervisor opens
it the same way. A command reads nothing on its input.

### Its leader, reaped last

`waitid` with `WNOWAIT` sees the program exit without reaping it, and
its leader is reaped only once its group is killed, so the group's ID
can't be another's when it's killed. A background supervisor waits the
same way.

### Its output

Each of its output and its errors keeps its start and its end, half of
`--output-bytes` each, and how much there was. The answer's text holds
them once. Its structured value has the exit, the sizes and the time,
not the text again, and a background `read` likewise. Calls on one
session run at once, each on a thread of its own, at most
`calls_at_once` of the contract's `limits.toml`.

### Its output's clamp

The agent gives `--output-bytes` as `[tool.run] output`, clamped by
`link_run_tool_contract::output_within` to half of the `task`
namespace's answer cap less room for the fences, so the errors' end is
never what the harness cuts. `serve` refuses more than `output_most` of
the contract's `limits.toml`, which keeps both streams, each byte shown
as up to three, under the tools bridge's message cap.

### Other limits

The same file holds the rest the tool holds to: the most bytes of a
call's command (`command_max`), the shell (`shell`), how long a command's output is drained once it exited
(`drain_ms`), and a background command's timings, how long `stop` waits
(`stop_wait_ms`), how often its supervisor looks (`tick_ms`), and how
long a sweep tries to tell of its end (`tell_for_secs`).

### `tests/run/command.rs`

It runs the tool without a sandbox, and on Linux under `lnk-sandbox
run` from the sandbox module.

## Background commands

`src/background.rs`. With `--state` (and `--way-in`, `--as <agent id>`
with `--keyring <dir>` or Link's keyring, `--background`,
`--background-timeout`, `--background-output`, `--background-keep`,
`--hops`), `command` takes `run_in_background`, and `serve` offers
`list`, `read` and `stop` too. Without them, `run_in_background` is
left out of the schema `tools/list` gives, and a call giving it is
refused.

### A background start

It checks the call as any `command` does, its `timeout` at most
`--background-timeout`, then, under a lock on
`<state>/start.lock`, counts what runs, writes the record
(`<state>/<id>.json`, naming its schema, outside the files folder: no
sandbox writes it) and starts `lnk-run-tool supervise <id> --state <state>`. The conversation and
hops come from the call's `_meta` (`link.local/thread`,
`link.local/hops`): without a conversation, or at `--hops`, it's
refused.

### The record's lock

A start takes the record's lock (`<id>.lock`, `flock`) first and hands
it to the supervisor as its input, so no sweep ever sees it free while
it starts.

### `supervise`

It holds the lock for its whole life, which the kernel releases however
it dies. It runs the sandbox in a process group of its own, with
`PR_SET_PDEATHSIG` on Linux, so the sandbox, which bubblewrap ends with
it, dies with the supervisor.

### `gate`

The sandbox starts through `lnk-run-tool gate`, which waits on a pipe
until the record names its group, then becomes the sandbox, the same
process: a supervisor killed before then leaves nothing running that a
sweep can't find.

### Its output file

Its output goes to `background/<id>.out`, opened with `openat` and
never through a link. Past `--background-output`, the newest half is
copied beside and moved in place, and the record counts what was
dropped, for the file it names (its inode): a `read` that finds another
file, cut since, loads the record again, so it never reads at the old
offsets.

### Its end

It stops the group at its timeout or on `<id>.stop`, records the end,
then hands the bridge's way in a message from `run:<id>`, a bridge's
call proved as its agent (`--as`) with the agent's key from the
keyring, read then. The record is marked told before, and unmarked only
when the way in can't be reached. SIGTERM, SIGINT and SIGHUP, the
agent's service stopping, end it lost, untold.

### `sweep <state>`

Run by the bridge as it starts and by `serve` at each start, it takes
each record's lock. A free one means its supervisor is gone. A running
record is then marked lost, and what's left of its group is killed when
it's surely its own: the same boot, and its leader the one recorded or
gone. Ended records are told, tried again for ten minutes, and those
told past their keep measured from the end are removed with their output.

### `stop --state <dir>` and `remove --state <dir>`

`stop`, by [`lnk agent stop`](../../../../../../../agent/docs/agents/harnesses/use/README.md#one-running-in-a-terminal), and `delete`, by [`lnk agent delete`](../../../../../../../agent/docs/agents/start/create.md#delete-an-agent) or a move,
stop what runs, and `delete` then deletes everything.

### `tests/run/background.rs`

It runs them as real processes: outliving the call and its server,
told once through a stand-in way in, a stop leaving nothing, a
supervisor killed or sent SIGTERM, the caps, the keep, and on Linux all
of it under `lnk-sandbox run`.

## Provider Calls

The Run server publishes `command` where the shell is granted and, when background settings are present, `list`, `read`, and `stop` beside it. Managed agents use the `task` namespace so command work and scheduler jobs share discovery while retaining their provider policies.

### Direct Background Start

A direct MCP client calls the provider’s `command` with `run_in_background`. Agents call `task__run` the same way, described in [Start a Background Command](../guide.md#start-a-background-command), with their existing command grants.

### Direct List, Read and Stop

Direct clients call `list`, `read`, or `stop` on this provider. Agents select `kind = "run"` on the corresponding `task` action when scheduler jobs are also available, as the [guide](../guide.md#list-read-or-stop-background-commands) explains.
