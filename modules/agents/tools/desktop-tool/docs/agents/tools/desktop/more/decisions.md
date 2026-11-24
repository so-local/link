# Decisions

Why [Link Desktop](../README.md) gives an agent a virtual desktop of its own.

## Why a virtual display rather than the owner's screen?

An `Xvfb` display works on a box with no screen and touches nothing of the
owner's, so an agent can use it unattended. The tool never reaches the
owner's own screen, and no setting turns that on: a real screen would need
the owner's permission for every action, and the desktop portal and
capture interfaces it would use cannot be exercised on a cloud machine.

## Why is there no window manager?

A window manager draws frames and title bars and moves, raises, and
closes windows. The desktop runs none, so it stays one `Xvfb` with
nothing more to confine or keep running. Programs' windows have no
frames and no key closes one, so the tool says so rather than suggest
`alt+F4`. A minimal window manager can come later if a program needs
one.

## Why does each display have a cookie?

The display starts with an MIT cookie, so no other user of the machine
draws on it. The cookie is renewed at each start, and sent only to an X
server run by your user, since another user's server started with
`-ac` takes any client and would be handed it, and a root one, such as
the computer's own X server, would see the agent's screen. Each program the agent's `run` tool starts gets that
display's `DISPLAY` and `XAUTHORITY`, and its sandbox reads that display's
socket and the cookie, never the whole of `/tmp/.X11-unix`.

## Why is the display picked by Link?

Link selects a number with no X server socket or lock file when the
desktop is added, reserves it in your user's runtime folder, and keeps
it for later starts, so none of your other agents' desktops takes it.
Another user's X server can still take the number meanwhile: the tool
asks the kernel who serves the socket before sending the cookie, and
refuses a server that isn't your user's.
[Troubleshooting](../troubleshooting.md#display-120-is-taken) explains
how to select another number.
