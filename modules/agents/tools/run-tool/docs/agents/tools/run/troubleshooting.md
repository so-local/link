# Troubleshooting

What to do when [your agent can't run a program](README.md), reach a host, or keep
a command running. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk agent show
```

## Your agent says it can't run programs

Its sandbox is off (`--no-sandbox`), or `[commands]` doesn't grant the
shell (the `shell` toolchain, `sh`, or `allow = ["*"]`), so it's offered
no `run` call: every command runs in the shell.

## A program in a command is refused

The program isn't in its sandbox file's `[commands]`, so the sandbox
refuses to run it though the shell may run. Add it there, then
[`lnk agent start --sandbox`](../../../../../../agent/docs/agents/README.md#create-an-agent) your file.

## A command can't reach a host

Its sandbox reaches only the hosts it's granted. `lnk agent permission add
--internet.hosts` the host, or `'*'` for any.

## "stopped after ..., still running at its timeout"

A command runs for at most two minutes by default, or less where its
call's `timeout` asked. Give it more with a
larger `[tool.run] timeout` in its specification, up to its `[tools] timeout`
(five minutes by default), where every call stops waiting. Above it,
your agent doesn't start and says to raise `[tools] timeout` too.

## A long command keeps stopping at its timeout

Ask your agent to run it in the background (`task__run` with
`run_in_background`), where it
runs for up to an hour by default, or as long as `[tool.run]
background_timeout` says.

## "... background commands run already"

Four run at once by default. Wait for one to end, stop one, or raise
`[tool.run] background`.

## A background command was "lost"

Your agent or your computer stopped or restarted while it ran: ask your
agent to start it again.
