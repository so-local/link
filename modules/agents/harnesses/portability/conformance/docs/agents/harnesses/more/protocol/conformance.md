# Conformance Kit

`link-harness-conformance` checks that a harness program answers the
[contract](../../../../../../contract/docs/agents/harnesses/more/protocol/contract.md),
by running it as `lnk-agent` would, without Link. Use it in adapter tests to exercise installation, configuration, and endpoint behavior against the kit’s controlled fixtures. Those checks establish the selected fixture’s behavior; native runtimes still need their own evidence.

## What it runs

`check` runs the adapter's `contract`, `install`, `configure`,
`command` and `health` with its home as `HOME`, and `needs` outside its
proxy and behind it (`proxied`), then checks its endpoint against a
bare chat completions model of the kit's own. `try_check` gives the
failure back instead of failing the test.

### One provider only

`Check::new(adapter).provider(...)` checks a harness that takes a local
model only one way with that provider, as an adapter that runs local
models only through Ollama is checked with `ollama`.

### A stand-in harness

`Check::new(adapter).stand_in(program, files)` checks an adapter
without downloading its harness: the harness fixture, `lnk-harness-fixture`, built in the kit's workspace, is written at `program` in its home and
run whenever it's given `acp` among its arguments, and each of `files`
is written there first, so the adapter's `install` finds its harness
installed. It asks the kit's model. What the adapter does around its
harness, its hub, its key and its threads, is checked as it runs.

### Additional Entrypoints

`stand_in_entrypoint(program)` adds another published entrypoint for the same
fixture. A name ending in `-acp` starts Agent Client Protocol (ACP) without a subcommand, while the
ordinary CLI fixture still answers configuration commands separately.

### Foreground Services

`stand_in_prelude(script)` adds fixture-owned command handling before that
dispatch. A stand-in with a separate foreground service can publish its native
readiness and handle shutdown, so the adapter's ownership is exercised rather
than bypassed. The script runs only in the check's private fixture home.

### Its downloads

The adapter's `install` and `configure` get this machine's proxy
settings, as a real harness's downloads need.

## What it checks of the endpoint

For an adapter with an endpoint, one whose harness speaks no
connection, it checks each part of the endpoint the contract says.

### Its key

The endpoint refuses a request without the key, or with another. When
the contract says `proof`, it proves it holds the key before it's sent
one, and sets `TCP_NODELAY` on the connection it accepted, read from the
harness's own socket where the system says.

### Who a message is from

When the contract says `from`, it answers a message from someone who
isn't the owner (`Link-From`: a schedule, a background command, someone
in a shared chat and their name), telling the model who. It refuses one
from no sender a message may be from, or a name that isn't a person's.

### Which agent

It answers requests naming its agent in `Link-Agent`, as Link's bridges
send them. When the contract says `agent`, it refuses one naming
another agent before the model sees it, and answers one naming none as
its own agent's.

### Answers and threads

It answers, streamed and whole, with what the model said, keeps each
thread and keeps threads apart. A turn cut short, its caller gone
before the answer, runs to its end, and the answer is kept in the
thread.

### Its tools

It calls a tool through the bridge: the kit's own MCP server, given as
Link gives a tool, in the settings' `tools`, which the hub gives each
session as its `mcpServers`, and at `$LNK_TOOL_KIT`, called when the
model asks, its answer sent back to the model. A stand-in for a harness
that keeps Link's tools in its own settings (`Check::tools_in_settings`)
finds them at `$LNK_TOOL_KIT`, as such a harness's settings give them.

### Narrowed tools

When the contract says `tools`, a thread narrowed to some tools
(`Link-Tools`) is offered only those from then on. A later list narrows
it again and never widens it, a message without one keeps it, a call
says the thread's tools in its `_meta` (`link.local/tools`), and a list
that isn't one is refused.

## What it checks of the sandbox's side

The sandbox points the harness at its proxy in `HTTPS_PROXY`, the run's
secret in the address, and hands it the machine's CAs as they were set.
The `Launch` that `command` prints sets no proxy variable of its own
(`HTTPS_PROXY`, `NO_PROXY` or another casing), empties no CA variable
(`SSL_CERT_FILE`, `NODE_EXTRA_CA_CERTS`) and, when it sets
`NODE_USE_ENV_PROXY`, sets it to `1`, so Node reads the proxy.

## What it checks of agent state

When its `initialize` advertises agent state that gives and takes
sessions, its export holds a thread it answered, by what was said, and
a conversation it imports keeps its thread's name and goes on: the
model's next request holds what came in.

### Personal Continuation

The four personal destinations each import the other three source fixtures,
covering all twelve directed pairs. The scripted model answers the next
decision only when its input includes the corrected choice and unfinished
invoice. The check also verifies exact archive prefixes and no historical
tool execution. This proves adapter input and archive behavior, without
claiming live native-runtime or model evidence.

### Without Native Import

`Check::without_native_state()` makes the harness fixture advertise no Agent State Protocol (ASP).
OpenClaw and DeepSeek use this mode to exercise their adapter stores and
the shared portable continuation path. Native codecs separately test
their pinned formats, including compacted and compressed records.

## Beside the kit

The kit carries what a connection's tests and an adapter's maintainers
need to run a harness without Link.

### An agent on the mini harness

`agent` is an agent on the mini harness, `lnk-mini-harness`, a harness that
speaks no connection, started in the foreground against a bare model,
for the tests of a connection's module against its fake: ports of its
own, servers on `127.0.0.1`, the agent's binding, and whether
bubblewrap runs.

### The connection fixture's service

`fixture_connection` is the connection fixture's service, faked: what `lnk-connection-fixture`
talks to, for a test whose agent needs a connection but whose subject
isn't one. Its bots are its tokens', each with the messages people sent
it, which a poll confirms as Telegram's offset does, and what it sent.

### Controlled runtime comparisons

The Rust [runtime comparisons](runtime.md) use the same local services for
Mini, full Link and two agents. They retain Measure's typed phase records,
separate traced counts from timings, and report confinement gaps explicitly.
