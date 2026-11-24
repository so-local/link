# Troubleshooting

What to do when [your agent](../README.md) doesn't install, start, stay running or
answer. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk agent show
```

## "the … installer Link downloaded isn't the one it pins"

A bad download, often a rate limit. Nothing it downloaded ran, and
nothing changed: try again later.

## `start` offers no local models

No model runs on this computer yet: its fix is on [the model's
troubleshooting](../models/agent/troubleshooting.md#start-offers-no-local-models).

## Linux: "the sandbox needs bubblewrap"

Install it, `sudo apt install bubblewrap`, then start your agent again
in its sandbox.

## Ubuntu: bubblewrap can't make the sandbox

Let `lnk agent start` add the AppArmor profile, which lets bubblewrap
make the sandbox.

## Linux: the agent stops when you log out

Run `sudo loginctl enable-linger $USER`, so your services keep running
once you log out.

## `start` says a socket is past the bytes a socket's path may be

Link's sockets are under your home, and a home's path over about 48
characters makes them too long: run it as a user with a shorter home.

## Linux: `start` says "no systemd for your user here"

Over SSH, in WSL or a container: run `sudo loginctl enable-linger
$USER` and log in again, or `lnk agent start --foreground`.

## `status` says it isn't answering

Read `lnk agent log show --follow`, then the [Link
Harness](../../../../harnesses/link/docs/agents/link/troubleshooting.md)
page, or its harness's own page.

## Getting hallucinations, or it forgets what was said earlier

Its window may be full, and what left it summarized. A larger `[window]
max` in its [specification](../../config/specs/agent.md)
helps, at a higher price per message.
