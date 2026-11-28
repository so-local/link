# Agent Bridge

What `lnk agent` runs beside an agent's harness to carry its
connections and serve Link's tools, as the [connection
contract](../../../../../connections/contract/docs/agents/connections/more/protocol/README.md)
types them.

## Owner Questions

The bridge delivers each question your agent asks you, and `lnk agent
question list` reads and answers them: [Owner Questions](questions/README.md).

### Retained Questions and Children

The `question` namespace creates, reads and stops questions for the authenticated calling work. A child question carries its own continuation and the original owner route. Pausing a child retains its progress and answer destination; partial text is withheld as a completion while execution is fenced. Released delegation proposals require review of the exact retained job and the current plan before `task reconcile` keeps or abandons them.

### Retained Result Delivery

Opted-in source turns retain a host result receipt until the bridge confirms delivery. The bridge keeps pending answers under its protected `owed/pending` directory, preserving the original destination across restarts. A parent accepts a stable delivery identity into its transcript once, even while it is paused; resumption processes the retained input. Parent acknowledgement does not wait for its next model response. Conversation and task-record deletion remove queued deliveries before their completion receipt.

## Run and file

A start defines the agent's run, file, code and scheduler providers under
their existing confinement. Its harness receives namespace MCP servers:
`task` combines runs and schedules, `file` combines file operations and
code lookups, and `agent` sends messages to teammates. Namespace servers
check each provider's current approval policy before forwarding.

### A sandbox per call

Each serves outside every sandbox and runs each call as `lnk sandbox
run` with flags made from the agent's sandbox file, a new sandbox per
call: Home and the own folder's `instructions/`, `skills/` and
`chats/` read and its `[files] write` written (`lnk-file-tool
apply --agent`: a path's first part naming one of those three is in the
own folder).

### `run` and `file`

For `run`, its network is as `[internet]` and `[tools]` say, `read =
["~"]` as `--files.read ~`, and only its `[commands]`; `run`'s `--timeout` is
the specification's `[tool.run] timeout` less a tenth (2 s at least, half at most), and `--output` its
`output`. For `file`, no network and only `lnk-file-tool apply`, which
reads the call on its stdin.

## The scheduler's runner

`lnk agent bridge` runs the agent's scheduler's runner (`lnk-scheduler-tool
run`) beside its connections, again each time it stops, and ends it as
it stops carrying the agent. Where one bridge carries every agent in a
time zone, one runner serves them all (`run --agents`).

### How it ends

It's asked with its stdin closed and a SIGTERM, on which the runner ends
in order (its firings ended, its mark on its store let go), killed past
10 seconds, and waited for. A runner ends by itself, too, once the
process that started it is gone, and on Linux is sent a SIGTERM then.

### `link_agent::kept`

The one way the harness keeps a program running, its runners' and
memory's model server's alike. A start in a terminal stops it once its
harness ends.

### With `driver = "cron"`

Where the specification's `[tool.scheduler] driver` is `cron`, `lnk agent start`
has the machine's calendar start it once a minute,
`lnk-scheduler-tool cron install --label <agent id>` with the same
flags, keeping `cron uninstall`'s command in
`~/.lnk/safe/agents/<agent>/scheduler-cron.json` for `lnk agent stop`.

### `lnk-run-tool sweep`

Where the agent has `run`, `lnk-run-tool sweep` runs once as it starts:
background commands lost while it was down are marked, and each one's
end not yet told is sent through the way in.

### Proving each firing

The runner and `run` prove each firing as their own agent alone (`--as
<agent id>`), with its key from the keyring, read at each firing.

### Its flags

The scheduler's tool is `scheduler-<agent id>`, its runner and tool
given the agent's specification's `[tool.scheduler]` as flags
(`link_scheduler_tool_contract`, which any machine running an agent's
scheduler gives it the same way): `--per-day`, `--subagents`, `--hops`,
`--late`, `--time-zone`, and the runner's `--timeout`, from `firing`.

### On a team

For an agent on a team, its ID, teams and teammates (`--agent`, `--team
<name>=<id>`, `--teammate <team>/<name>=<agent id>`), and the runner each
teammate's way in here (`--way-in-of <agent id>=<socket>`).

### Through a team's relay

At each start the runner is also given its team relay when an account
that hosts agents carries one of the agent's teams, as that account's
`agent-team list` says: `--team-relay <url>` and, per team it carries,
`--team-owner <team>=<account>`. A teammate on none of this machine's
ways in is then reached through the relay, signed in as the agent. An
account that doesn't answer is said in the bridge's log, and the runner
starts without it.

### With `ask`

With `ask`, its `task`, `schedule`, `send` and `cancel` aren't allowed
when it's defined, so each asks first, in [`lnk sandbox question list`](../../../../../../security/sandbox/docs/tooling/sandbox/tools/questions/README.md#answer-what-a-sandbox-asks).

## Questions on your chats

The agent's bridge delivers agent questions, sandbox reviews and webhook consent on your direct chats. A separate response code targets each immutable request. Code alone confirms a singleton yes/no question. A permission yes waits for fresh source checks and application, while an ordinary yes returns information without authorizing execution.

### Where it asks

Where a turn of yours is under way, it asks on that connection's direct
chats, even for a turn in a group, or in `lnk agent chat` itself. With
no turn of yours, as for a webhook's question or a schedule's turn, it
asks on every connected connection, and once one answers, the others are
told how. Each connection's module says which chats are yours (`lnk-<connection>
owner`).

### Answering

A reply is caught where every message to the agent passes, before the
harness, so the agent never sees it. In `lnk agent chat`, a question
shows while its turn runs, and typing its code answers it there; any
other line waits for the turn to end. Text, choices and set clarification use the shared code-prefixed grammar. `[connections] ask_wait` supplies the default deadline, with typed interaction opinions for information fallbacks. Permission expiry always leaves access denied. A partial submitted set stays partial and never acquires later automatic answers.

## `lnk agent bridge`

Hidden. Carries the connections of the agent `--agent` names as
`~/.lnk/safe/agents/<agent>/bridge.json` says (`{"endpoint", "key", "proof",
"connections", "from", "recover", "tools", "parts"}`, written by `lnk agent
start`), a `lnk-<connection> bridge` for each, started again when one
stops. Read by the agent's bridge service (`link.bridge.<agent id>`,
`lnk-bridge-<agent id>.service`).

## `lnk agent bridges <dir>`

Hidden. Where `~/.lnk/safe/agents/policy.toml` says `shared_bridge = true`, carries every
agent recorded in `<dir>/agents/<agent id>.json` as `lnk agent bridge`
would, each from its own `bridge.json`, each agent carried again on its
own when its `bridge.json` or record changes. With no agent recorded for
20 seconds, it ends.

### Each record

`~/.lnk/safe/agents/bridges`, `{"name", "runner", "foreground", "sleep"?}`,
written by `lnk agent start` in place of the agent's bridge service, and
removed by its stop.

### Its runners

It runs one `lnk-scheduler-tool run --agents <file> --time-zone <zone>` a
time zone for the agents' runners (each record's `runner`, `<program>
run <flags>`), `<file>` (`<dir>/runners/<hash of the zone>.json`)
listing each agent's flags.

### `sleep`

A record with `sleep` (`{"port", "net", "after_ms", "run",
"conversations", "job", "handoff", "shared"?}`) is an agent whose `[run]
idle_after` lets it stop. The bridge listens on its endpoint's `port`,
always without `handoff` (Linux, behind its proxy, which then doesn't
serve it), passing each connection to `<net>/in.sock` (its port as two
bytes, big-endian, then the bytes both ways).

### `handoff`

With `handoff`, its harness listens there itself, so the bridge listens
only while it's asleep, letting go of the port at the first connection
and passing it on once the harness listens.

### Waking it

It starts its `job` first when it's asleep (`{"service": {"label",
"unit"}}`, `link_service::resume`: launchd's `bootstrap` or
`systemctl --user start`; or `{"terminal": "<socket>"}`, a line `wake`,
answered `ok`), and the harness its owner's agents share (`shared`) if
none runs. `link_connection_contract` tries a refused connection to an
agent again for 5 seconds, for one handed its port as it wakes.

### Putting it to sleep

It stops it the same way (`pause`, `sleep`) after `after_ms` with
nothing through or connected, no answer still coming through it, no
`turn.json` in `chats` and no lock held in `run`, marking it
`<dir>/asleep/<agent id>`. An answer still coming is read from each
connection's HTTP/1.1 framing: an ask unanswered, or an answer's body
not all through, or what it can't frame, until the connection ends.

### Where it runs

Run by its own service (`link.bridges`, `lnk-bridges.service`) where
Link's services run, else in a session of its own, which an agent
started in the foreground starts again when it's gone.
