# Developing

`lnk-desktop-tool` is Link Desktop: an MCP server over stdio whose
calls see and work an `Xvfb` display, for any harness to reach as a
tool. How to use it: [Link Desktop](../README.md).

## How it speaks X

It speaks the X protocol itself, through `x11rb`, with no Xlib: it
connects to `/tmp/.X11-unix/X<n>` with the cookie in `--xauthority`'s file
(an `Xauthority` file of one entry for any host, made by
`lnk-desktop-tool cookie create`), and starts `Xvfb :<n> -screen 0 1280x800x24
-auth <file> -nolisten tcp -nolisten local -noreset -nolock` when nothing
answers there, killed when the tool ends.

### Its deadlines and size

Every wait for the server has a deadline, `reply-ms` in
`src/limits.toml`, beside its start and stop deadlines, a drag's pacing,
the folders `Xvfb` is looked for in, and the display's lock. The
desktop's size is there too, `width` and `height`: Xvfb's screen is made
from them, and each `[x, y]` a call takes is held to them.
`actions.toml` bounds each axis by the larger side, as one list's items
share their bounds, and a test checks it agrees. Key names are in
`src/keys.toml`.

### Who gets the cookie

Before the cookie is sent, the tool asks the kernel who is at the
other end of the socket (`SO_PEERCRED` on Linux). It sends the cookie
only to a server run by your user, never root, and, for the server it just
started, only to that server or a process it started, found through
`/proc`'s parent links. Each server it starts gets a new cookie first,
written beside the file and moved over it, so a reader finds one
cookie whole. It prepares `/tmp/.X11-unix` before starting `Xvfb`,
including when another process creates the directory concurrently.

### Xvfb in its sandbox

`Xvfb` runs under the sandbox given after `serve`'s `--` (`lnk sandbox
run`), with `--files.read` of its cookie and of its program's folder
outside the system's, and `--files.write /tmp/.X11-unix`, so its socket
is made where programs reach it. The tool holds `/tmp/.X<n>-lock`
itself, since the sandbox's `/tmp` is its own, and removes the lock and
socket when `Xvfb` ends.

### Screenshots and input

A screenshot is the root window's pixels (`GetImage`, 32 bits a
pixel) made PNG here; input is XTEST's `FakeInput`; a keysym no key
gives is put on a key no keysym is on (`ChangeKeyboardMapping`). A
call reads the keyboard mapping once, looks up all of a combination's
keys before pressing any, and releases what it pressed however it
ends. What `Xvfb` writes on its errors is read all along, its end kept
to say why a server ended.

## Build and test

From the repository root, run `cargo build -p link-desktop-tool`, then `cargo test -p link-desktop-tool --test desktop`.

### `tests/desktop.rs`

It starts the tool on a free display, maps a window of
its own there and checks what the window gets: the clicks at its
pixels, the characters typed, a combination held, the wheel's buttons,
and its color in a screenshot. It checks that the server took a new
cookie, and it ends the server under the tool to see the next call
say so. `Xvfb` runs in `lnk-sandbox`, built
beside the test. Without `Xvfb` on PATH, or a sandbox that works on the
machine, it reports an explicit skip.

## Direct Tool Setup

Manual Desktop registration supplies its display number and authorization cookie. The managed [Desktop workflow](../guide.md) chooses a free display and registers the tool for the agent.

### By hand

[`lnk agent tool add desktop`](../../../../../../../agent/docs/agents/README.md#add-tools-and-skills) is the module, the display's cookie, the
tool shared by every session of the agent with its calls allowed, and
your agent allowed to call it. After adding the module, make the
display's cookie with `lnk-desktop-tool cookie create
~/.lnk/data/agents/main/desktop/auth --display 120`, the module's own
program, before the tool is added.

```sh
lnk mod add desktop-tool
lnk sandbox tool add desktop --shared --add screenshot --add click --add type --add key --add scroll --add drag -- lnk-desktop-tool serve --display 120 --xauthority ~/.lnk/data/agents/main/desktop/auth -- lnk sandbox run
lnk agent permission add --tools.names desktop
```
