# Stop and Restart

Stop your agent and everything started for it, until it starts again.

```sh
lnk agent stop
lnk agent restart
```

Normal `lnk agent start` also registers automatic startup. On macOS,
launchd keeps it running and starts it at your next login after reboot.
On native Linux it enables the user service. `lnk agent stop` removes
that automatic startup as well as stopping it now. `restart` applies
stop and start to an existing agent; it keeps its saved model, files
and tool configuration. `--agent <name>` selects another agent.

## In a Terminal

One started with `--foreground` is `running in a terminal` to
`status`, and stops with Ctrl-C there, `lnk agent stop`, or a SIGTERM or
SIGHUP to `lnk`, which it passes on to the harness and waits for (one
asleep for being idle just ends). On Linux, `lnk` killed outright
(SIGKILL) sends its harness a SIGTERM all the same, so nothing it
started keeps the agent's ports. Until then another `lnk agent start`
says it runs there. Its output shows there and goes to its log too, as
a service's does, so `lnk agent log show` reads it either way.

### How Long a Stop Waits

`lnk agent stop` waits up to 35 seconds for an agent in a terminal to
end: the 30 its harness has to finish the turns it's answering, and a
moment to exit. Only then does it say the agent didn't stop.

## What a Stop Ends

A stop ends everything started for the agent, however it runs (in a
terminal, as a background service, or woken when a message comes):
its harness, its bridge and connections, its scheduler's runner and the
calendar's wake, and its background commands. Nothing of it fires a
job or answers a message until it starts again.
