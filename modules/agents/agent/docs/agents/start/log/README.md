# Agent Log

Your agent's log is what its harness and Local Link wrote while it ran. It says what
the agent answered on, which tool failed, and why the agent stopped. Read the log when the
agent doesn't answer or answers slowly. [Check Your
Agent](../status.md) says first whether the agent runs.

## Read the Log

`lnk agent log show` shows what your agent's harness and Local Link said since the agent's
last start, at most the last 200 lines. Each line starts with its time on
your machine's clock, with its date when it isn't today. Then comes `!`
for a warning or `✗` for an error, and what was said. After that, faint,
are the IDs that place the line, cut to their first eight characters:
the conversation (`chat`), the turn, and the tool `call`. A line
written several times in a row shows once, with how many times.

```text
14:05:11   Link Harness answering on 127.0.0.1:31062
14:05:40 ! tool browser left out of this turn: it failed to start  ×3
14:05:41 ! Tool agent__send failed: tool agent answered 502 Bad Gateway  chat link:3e75d6ac… · call toolu_01…
```

`--follow` keeps printing lines as they're written. `--json` prints each
line's record, one JSON object a line, with its time in UTC.

```sh
lnk agent log show --json
```

<!-- json: link_format::log::Line -->
```json
{"at":"2026-11-09T14:05:41.042Z","level":"warning","agent":"3e75d6ac33d355c208c3e1140708b8a9","chat":"link:3e75d6ac33d355c208c3e1140708b8a9","call":"toolu_01ABCDEFGH","text":"Tool agent__send failed: tool agent answered 502 Bad Gateway"}
```

A harness other than Link Harness writes its own lines. In a terminal,
`lnk` gives each line its time as it logs it. Run in the background, the
lines are kept as the harness wrote them, and show with no time, as `{"text"}`
with `--json`. [Format](../../../../../../../packages/format/docs/more/libraries/runtime/format.md#an-agents-log) says how the log is written.

## Watch the Log Live

`--live` prints a line a step. The steps are a message come, the model thinking (and each minute it goes on that it still is), a tool called and answered or
failed, the answer with how long it took and its tokens, and the agent
stopping or starting. A slow answer then shows where it waits: on the
model, on a tool, or on nothing. `--live` reads Link Harness's conversations,
so with another harness it shows only starts and stops.
