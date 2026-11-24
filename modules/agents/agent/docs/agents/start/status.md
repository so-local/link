# Check Your Agent

Ask whether your agent runs, and follow what it does.

```sh
lnk agent show
lnk agent log show --follow
lnk agent log show --live
```

`status` says one of five states. **running** means its harness answers,
and **running in a terminal** that it runs there, from
`lnk agent start --foreground`. **asleep** means it stopped for being
idle and its next message starts it. **stopped** means it isn't running.
**not answering** means it runs but its harness doesn't answer, and
`lnk agent log show` shows why. Asleep and the two running states exit with success, the others with a failure.

## The Log

`lnk agent log show` reads what your agent's harness and Link said, and
`--live` follows each step as it happens: [Agent Log](log/README.md).

## `status`

`status` succeeds for running and asleep agents. When its proxy has
paused its wire, or every wire is stopped, it says so and why. On a box,
where no notification shows, that's where you see it. For other
programs, `status --json` and `list --json` are in
[contracts](../../../../../../docs/more/modules/contracts.md).
