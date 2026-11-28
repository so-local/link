# Desktop Guide

Enable Desktop on Linux to give your agent a virtual screen, mouse, and keyboard. Install `Xvfb` first, then use the tool with programs started by Run.

```sh
lnk agent tool add desktop
```

## Add it

It installs `desktop` if it's missing, picks a display number no
X server here uses and no other agent's desktop keeps, makes the
display's cookie, and restarts your agent with the tool. [`lnk agent tool remove desktop`](../../../../../../agent/docs/agents/README.md#tools) takes it away.

### Its display number

The number is the agent's while it has the desktop, even while the
display is down, because it's reserved: a file naming the agent's
desktop in `agent/displays` of your user's runtime folder (on Linux,
`/run/user/<uid>/lnk`), which no other of your agents' desktops takes
and no other user writes.

#### When a number is lost

Each start checks the reservation again. A number lost
meanwhile (the reservations go when you log out or the computer
restarts) and taken by another agent is given up for a new one, with a
new cookie. Another user's desktops reserve in their own folder; a
number their X server holds is passed over, and one whose X server
started first keeps it.

### In its specification

[`lnk agent tool add desktop`](../../../../../../agent/docs/agents/README.md#tools) also writes `enabled = true` under
`[tool.desktop]` in the agent's specification, and `remove` writes `false`. An
agent whose specification says it gets its desktop at its next start on a Linux
machine, [its module installed if it's missing](../../../../../../agent/docs/agents/tools/use/decisions.md#why-can-a-specification-turn-a-browser-or-desktop-on).

### What it needs

The desktop is a virtual one, on Linux: an `Xvfb` display of the
agent's own, so it works on a box with no screen and never touches
yours. You need the `desktop` module and `Xvfb`. On Ubuntu or Debian,
a box among them, `sudo apt-get install -y xvfb` installs it, and on
Fedora `sudo dnf install -y xorg-x11-server-Xvfb`. A
browser on the desktop is the [browser tool's](../../../../../browser-tool/docs/agents/tools/browser/guide.md#on-a-box-or-ubuntu),
which needs Google Chrome's Debian package on Ubuntu, never its
Chromium snap. An agent moved to a
machine that isn't Linux starts without its desktop, and says so; it's
back when the agent starts on Linux again.

### Its programs

Each program its `run` starts gets `DISPLAY` and `XAUTHORITY` for that
display, and its sandbox reaches that display's socket and cookie,
nothing more of `/tmp/.X11-unix`.

### Ask it something

Ask it to "open the spreadsheet in LibreOffice and total column C" and
it starts the program with `run`, takes a screenshot, and clicks and
types until it's done.

## What it does

Your agent calls it as `desktop__<call>`, with the arguments computer
use takes, the shape models are trained on: a pixel is `coordinate`,
`[x, y]` of the screenshot. In Link Harness, the owner can invoke these slash commands
with the same action’s JSON arguments and grants.

### `screenshot`

`/desktop:screenshot` gives an image of the whole desktop, 1280 by 800
pixels, which your agent
reads to find where to click.

### `click`, `scroll`, and `drag`

`/desktop:click` moves the mouse to `coordinate` and clicks: left,
right, or middle (`button`), once or twice (`double`). `/desktop:scroll`
turns the mouse's wheel `scroll_amount` clicks in `scroll_direction`, at
`coordinate` when given. `/desktop:drag` presses at `start_coordinate`,
moves to `coordinate` and lets go.

### `type` and `key`

`/desktop:type` types text where the keyboard's focus is, up to 4000
characters a call. `/desktop:key` presses its `text`, a key or a
combination: `Return`, `Escape`, `ctrl+c`, `ctrl+shift+t`, or an
xdotool name such as `minus` or `KP_Enter`. A capital letter among
a combination's keys is its key, so `ctrl+V` presses `ctrl+v`; add
`shift` to hold Shift. If a key can't be given, nothing is pressed, and
whatever was pressed is let go however a call ends.

### No window manager

No window manager runs on the desktop. A window manager is the program
that draws windows' frames and title bars and moves, raises, and closes
windows. Here, windows have no frames, and no key such as `alt+F4`
closes one. Your agent closes a program from its own menu or ends it
with `run`. Each call's pixels stay inside the desktop, 1280 by 800
pixels: one past it is refused, naming the desktop's size.

### How input is sent

Input goes through the X server's XTEST extension, so a program sees
what a person's mouse and keyboard would send. A character no key
gives, such as `€` on a US layout, is put on a spare key while it's
typed.

### Marked as the screen's

A screenshot's line comes inside a fence, an opening `<untrusted ...>` line and a closing line with the same random ID, which tells your agent's model that it's data to read and never your instructions, since anyone may have written it. An image can't be fenced, so the fence's note comes before it: what the screen shows is what its programs drew. [Untrusted Content](../../../../../../../../packages/untrusted/docs/more/libraries/runtime/untrusted.md) explains the fence.

## The display

The display starts at the first call and ends when the tool does, when
its last session has been idle a while: the programs on it end then
too. The agent's other sessions share it. A display that stopped under
the tool starts again at the next call, and that call says the
programs that were on it are gone, so your agent starts them again.

### Its cookie

Its cookie is a file only your user reads, and the display takes
no client without it, so no other user of this computer sees or draws
on it. It listens on its socket only, never on TCP. The tool gives the
display a new cookie each time it starts the display's X server, so a
cookie copied while an earlier display ran opens no later one.

#### A server already at the display

Before it sends the cookie to a server already at the display, the tool asks
the system who runs that server, and sends it only to one run by your
user, never root's: a computer's own X server at that number would
otherwise see the agent's screenshots and take its input.

### Its sandbox

`Xvfb` runs in a sandbox of its own, as the browser's Chromium does: no
network, and it writes only X's folder of sockets. The tool takes
`Xvfb` only from the system's folders (`/usr/bin`, `/usr/local/bin`,
and `/bin`), from a program and folder that only root writes, never
from your `PATH`, which may hold a folder your agent writes. An X
server that stops answering fails the call after 10 seconds rather
than holding the tool. The display is
still [a door between the agent's `run` sandboxes](../../../../../../agent/docs/agents/tools/use/security.md#desktop), which all reach it.


## Terminal Actions

`lnk agent desktop` exposes Desktop actions for the selected agent. Read an action’s JSON arguments from a file with `--request <file>`, or from standard input when the flag is omitted. The action uses the running agent’s configured tool and grants.

### Choose an Action

`lnk agent desktop screenshot`, `lnk agent desktop click`, `lnk agent desktop type`, `lnk agent desktop key`, `lnk agent desktop scroll`, and `lnk agent desktop drag` use the corresponding calls described above. Use `--agent work` to select another agent and `--json` for machine-readable output.

## Manual Registration

Use [developer setup](more/developing.md#by-hand) when supplying Desktop’s display and authorization cookie yourself rather than enabling it through the agent.

## More about it

To work on its code, see [Developing Link Desktop](more/developing.md), and when
the display doesn't start or stays black,
[Troubleshooting](troubleshooting.md).
