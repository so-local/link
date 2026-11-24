# Troubleshooting

What to do when [your agent's browser](README.md) doesn't start, reach a page or
keep a sign-in. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk agent show
```

## The browser's program

When the browser finds no Chromium, or can't start the one it found or
was named.

### "no Chromium here"

The tool found no browser it drives. Install Chromium or [Google Chrome](guide.md#on-a-box-or-ubuntu),
or [name its program](guide.md#name-its-program) in the agent's specification.

### "the browser starts Chromium 120 or newer"

The Chromium found is older than the browser starts. Update it from
your distribution or Google, or [name a newer one](guide.md#name-its-program) as `[tool.browser]
program`.

### `lnk sandbox: refused …: add it to [commands] allow`

The browser's program is a launcher script, which runs inside the
browser's sandbox and is refused each program it calls, such as
`/usr/bin/id`. Find the program the script starts and [name it as
`[tool.browser] program`](guide.md#name-its-program).

### "[tool.browser] program … is a script"

The program your agent's specification names starts with `#!`, so it would run
inside the browser's sandbox and be refused the programs it calls.
Name the program the script starts instead. A refusal that the setting
isn't an absolute path, isn't a file here, or isn't executable names
what to fix the same way. Your agent runs without its browser until a
start finds the setting sound.

## "the browser didn't start"

The answer ends with the browser's last words. Chromium from a snap
can't run in the sandbox: install it from your distribution or Google.
Known distribution launchers resolve to their browser engine, so startup
does not depend on host configuration scripts outside the sandbox.

### Find the denied resource

After a failed attempt, run [`lnk agent log show`](../../../../../../agent/docs/agents/start/log/README.md#read-the-log). Tool failures include the
tool name, chat and error. If Chrome's dialog remains, its report's
`Termination Reason` and framework path help identify the denial. Share
those diagnostics without tokens or passwords. The browser's resource
permissions belong to its module, so granting the agent your whole home
is unnecessary.

## "wasn't opened: refused"

The host is outside [the agent's internet hosts](guide.md#internet-hosts), which the browser
inherits. See them, then add
the host, or `*` for any:

```sh
lnk agent permission add --internet.hosts example.com
```

## "the browser's way out is paused"

The sandbox's proxy paused the browser after a page reached for what no
normal page does. See why, then open it again by the name the answer gives:

```sh
lnk sandbox list --paused
lnk sandbox start run-4242
```

## "is under another part of the page"

Something else, such as a cookie banner, covers the part, and the
answer names it. The agent closes or scrolls past it, then clicks again.

## "on this machine or its network"

The page is local, on this computer or your network, which the browser
reaches only where the agent's grants name its port or host, as
[Open a page on your network](guide.md#open-a-page-on-your-network) shows:

```sh
lnk agent permission add --localhost.ports 3000
```

## "there's no [3] on the page now"

A part's number is that of the last read, and the page changed since:
read it again for its numbers.

## A site doesn't remember the sign-in

Each agent has its own profile, which keeps its cookies; give the same
`--profile`. Only a conversation answering you uses it: a member's or a
stranger's turn opens in [a context of its own](guide.md#each-conversations-page) that keeps nothing.
