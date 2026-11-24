# Decisions

Why `lnk-hermes` runs Hermes Agent the way it does. Link's own
decisions about harnesses are on its [Agents
Decisions](../../../../../../../../modules/agents/agent/docs/agents/more/decisions.md)
page.

## Why carry its state over the Agent State Protocol?

Hermes keeps its own sessions and memory files, and its hub reads and
writes them where Hermes does: its `SessionDB` and its home's files.
Using Hermes’s native interfaces lets imported conversations appear in its session list and continue there. Exact portable records remain available alongside the native projection, with unsupported content retained as described in the [switching guide](guide.md#imported-sessions).

## When does Hermes get its packages?

During installation, before the program payload becomes read-only.
Hermes's package manager installs every supported extra, including the
Anthropic SDK; Link's bridge carries connections without their native
libraries. Configuration never installs packages, so selecting a new
provider cannot require writes to executable code. The sealed manifest
uses relative paths to keep the staged selection valid after publication.

## How does Hermes use a local model?

As a `custom` provider at the runtime's `/v1` address, for any local
runtime [`lnk model list`](../../../../../../../../modules/agents/model/docs/agents/models/README.md#list) finds, such as Ollama, LM Studio and llama.cpp.

## How do your connections reach Hermes?

Over the [Agent Client Protocol (ACP)](https://agentclientprotocol.com),
as they reach every harness: Link's bridge, then the [harness hub](../../../../../../../../modules/agents/harnesses/portability/hub/docs/agents/harnesses/more/protocol/hub.md)
in [adapter endpoint](more/runtime.md), then its published `hermes-acp` entrypoint over stdio.
That entrypoint serves the same ACP protocol without loading the interactive
CLI first. Hermes runs none of
its channels, so one bot's credential has one reader: a switch or a
move never has two harnesses polling it. Each chat is a Hermes session,
kept by the hub, whose key is Link's key for the start, which only the
bridge holds.

### Its gateway

The gateway owns execution and MCP discovery; the frontend is a separate ACP
client process. Link starts the private profile's gateway in the foreground
and retains ownership of that child. Readiness must name that same child.
On shutdown, ACP closes first, then the gateway receives its native planned
stop signal so it can drain work and close its databases. A gateway that
cannot close cleanly remains a failure.

### Its Shutdown Diagnostic

On a stop signal, the pinned gateway starts a diagnostic of its own: a
`bash` script listing processes and kernel messages, bounded by GNU
`timeout` and started in a session of its own so it outlives a service
manager's cleanup. Under Link that session escapes the process group Link
stops, and the sandbox refuses `timeout`, a program it never grants. A
guarded native substitution leaves the diagnostic out, since only Link
runs this payload and Link's own logs say why the gateway stopped.

#### Its Upstream Reproducer

The upstream reproducer runs `hermes gateway run` in a process group where
`timeout` can't run, sends it `SIGINT`, and finds the stop starting a
detached process anyway. A gateway whose supervisor owns its process group
should be able to turn the diagnostic off. Remove the substitution when
the pinned upstream offers that.

### Managed Frontend

The pinned frontend ordinarily starts a detached gateway. A guarded native
substitution makes a Link-managed frontend discover its existing gateway
instead, waiting for native startup admission. If that gateway dies, the frontend cannot silently create a daemon
outside Link's ownership. Standalone Hermes keeps its original behavior.

### Its tools over ACP

`hermes acp` runs the editor's toolset by default, a coding agent's.
Link sets its ACP's toolset (`platform_toolsets.acp`) to
`hermes-api-server`, the tools its gateway's agent had, so it keeps
them, its MCP servers with them.

## Who runs its scheduled jobs?

Link's scheduler, as for every harness whose contract says `from`: a
schedule's message reaches Hermes over ACP, said as the schedule's, and
its answer goes where the schedule says. Hermes's native gateway also
has a scheduler. Link disables its cron tools
(`agent.disabled_toolsets: [cronjob]`) so new portable jobs have one owner.

## Why use cached catalogues when a session opens?

Hermes 0.23.0 through 0.23.3 build a model picker during ACP `session/new`. On an empty
cache, that path downloads the public `models.dev` registry synchronously,
even for a local model. A slow registry can delay the session by its
15-second download deadline before any inference starts.

### The workaround

The adapter applies guarded source substitutions during installation,
before the payload is sealed and becomes read-only: ACP selects Hermes's existing non-blocking catalogue
option, and both the inventory and Ollama Cloud fallback respect that
option when reading the registry. Cached provider data,
curated fallbacks and the current model remain available. Explicit native
CLI refresh still allows network discovery. A changed or ambiguous source
refuses the install, which publishes nothing.

### Hidden Metadata Reads

Provider labels and credential discovery can read the registry while building
the same picker. A context variable makes every registry read within that
ACP operation cache-only, including helpers that ordinarily request live
metadata. Its token resets after errors. Independent requests and explicit
native refresh retain live discovery, and cached labels remain available.

### The upstream reproducer

An upstream reproducer starts `hermes-acp` with an empty registry cache,
selects a local model, then sends `initialize` and `session/new` while the
configured `models_dev.url` is slow. Session creation should use the
non-blocking inventory path without downloading that registry. Remove
the substitutions when the pinned upstream implements these changes.

## Why must model metadata avoid local-server probes?

Hermes sees Link's hosted-provider relay on loopback and otherwise mistakes
it for a local Ollama, LM Studio or llama.cpp server. Its custom-endpoint
fallback also probes Ollama's management API repeatedly when discovery
fails. Those unsupported requests are correctly refused and can pause the
agent after the sandbox's refusal threshold is reached.

### Hosted Relay Discovery

Guarded substitutions recognize only the loopback hosted-relay path, keep
metadata discovery on its allowed `/v1/models` API and skip local management
probes there. Native local models and external endpoints keep their existing
discovery behavior. The route restrictions and circuit breaker stay intact.
The pinned upstream reproducer is a hosted relay returning models without
context metadata: repeated context resolution must never call `/api/show`.

### A Model on This Machine

A model running here reaches Hermes at the address Link hands it, behind
Link's local-model route. The route serves inference and a few read-only
paths (`/api/tags`, `/v1/models`), and refuses the rest. Hermes's
server-type detection probes LM Studio's, llama.cpp's and vLLM's
management paths, and Ollama's `/api/show`. When detection fails it probes
again on later turns, until the refusals stop the route and every request
fails, inference included. The same guarded substitutions skip those
probes for the address Link handed Hermes. Another local server keeps its
discovery.

### Auxiliary Model Features

The same narrowly recognized relay remains eligible for Hermes's Anthropic
auxiliary client. Titles and other native auxiliary requests must reach the
protected model route instead of being rejected because its provider hostname
is hidden. Foreign endpoints retain their existing compatibility checks.

### Schema Metadata

Hermes's dynamic browser description checks vision support before any image
exists. An empty catalogue can trigger an unrelated registry download. The
adapter keeps description metadata local, with a context variable reset after
errors. Native auxiliary availability probes also stay local and try an
existing auxiliary client first. Native-only vision remains available.
Actual image and tool-result routing retains network discovery, with separate
context for concurrent requests.

## How does Hermes get a secret in its settings?

By variable: the model's address, which holds the proxy's secret for a
model on this machine, is in its `.env` (`LNK_HERMES_MODEL_URL`), and
its `config.yaml` names it (`model.base_url: ${LNK_HERMES_MODEL_URL}`), which
Hermes reads as it loads its settings; so `hermes config set` never has a
secret on its command line, where every user of the machine sees it.
Hermes leaves a variable that isn't set as it's written, so nothing
waits on the order `.env` and its `config.yaml` change in. Its tools'
addresses are written into `config.yaml` itself (600).

## How does a hosted model keep its context window?

Link passes the selected model's known window as Hermes's
`model.context_length`. A protected relay hides the provider hostname that
native metadata discovery ordinarily recognizes; probing it can delay startup
and fall back to a smaller window. The known value also carries an override
from the owner's models file. A window change invalidates configuration;
unrelated pricing or answer limits do not rerun the native CLI.

### Unknown Windows

Facts for another model are ignored. Unknown hosted windows and local model
runtimes keep native context discovery, and switching away from a known
window removes the previous override.

## Why refresh tool routes separately?

The hermes adapter follows the [native tool-route refresh](../../../../../../../../modules/agents/harnesses/docs/agents/harnesses/guide.md#refresh-native-tool-routes) policy, keeping changing endpoint addresses separate from structural model and provider configuration.
