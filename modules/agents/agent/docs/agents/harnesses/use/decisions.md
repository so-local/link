# Decisions

Why harness selection, installation and compatibility work this way.

## Harnesses

Link Harness's own are on its
[Decisions](../../../../../harnesses/link/docs/agents/link/more/decisions.md)
page.

### Which harness is the default?

Link Harness: you install only what you need, and OpenClaw is a large
install someone else wrote, most of the minutes before a first answer.
Link Harness installs nothing from a third party. OpenClaw, Hermes and
DeepSeek Harness stay a `lnk agent harness set` away, and a box installs Link
Harness too. The current default is selected when an agent names no harness; changing
a saved selection uses `lnk agent harness set`.

### Why does Link have a harness of its own?

So the smallest engine does the whole job: it answers you on your phone
and remembers what you said. It is one more harness beside OpenClaw and
Hermes, as its own workspace package. The conformance kit checks
adapter protocol behavior with controlled fixtures. Link Harness calls
models and tools without executing model-generated code inside its own
process. Its tools run outside it, and conversations persist through the
selected store. Native runtime behavior needs separate evidence.

#### It grows through tools

It grows mostly through tools: almost every new capability is a tool,
not code in Link Harness. Its runtime adds only what a tool can't be: an
event that starts a turn, an answer no request waits for, a turn that
waits on an event, and what a message does mid-turn. Everything else is
a tool in Link's tools.

#### Declared, not discovered

What an agent can do is declared, not discovered. Its specification and its
sandbox file decide it when the agent is configured, and the prompt and
the tools then stay the same from turn to turn, so the provider's cache
holds.

#### What's in it

Schedules, subagents, notes the agent writes, skills, web and compaction
are in Link Harness because without them it can't stand in for OpenClaw
or Hermes, and those are what people use agents for.

### Why Keep Harnesses Behind Adapters?

Each harness has its own installer, configuration, runtime requirements
and state format. An adapter keeps those mechanisms behind the same
contract, so agent management can select a harness without depending on
its private layout. The published adapters are separate workspace owners;
their security and runtime pages describe what each needs.

### Why does Link come with OpenClaw, Hermes and DeepSeek Harness?

So that the personal harness adapters are available with Link: `lnk agent harness set hermes`
works with nothing to add or trust first. Link's release builds the
three adapters from their workspace packages, at the core's version, and signs
them with Link's key, as it does its own modules; [`lnk upgrade`](../../../../../../../docs/more/modules/guide.md#upgrade) updates
them with Link.

#### Where they're named

They're named only in the core's module lists (`MODULES`, `needs`) and
the harness module's `KNOWN`, besides what it keeps for agents, so what's specific to each is still
its adapter's answer through the contract. A harness Link
doesn't come with comes from its own repository.

### How do the core and an outside adapter keep working together?

Through a versioned contract. They release apart, so a newer `lnk-agent`
must run an older adapter and the other way round. The adapter says
which version it speaks (`lnk-<name> contract`), and each change is
additive, as in the relay's protocol: a new command with what
`lnk-agent` assumes when an adapter doesn't answer it, or a new field
with a default. What one harness needs, such as how long it takes to
answer after a start or whether it takes the proxy's address, is the
adapter's answer, never a name in the core.

### How does a change to the contract land?

In `link_harness_contract::harness` first, as an addition an older side
can follow. The mini harness answers it, the conformance kit checks it,
and then the harnesses take it up, each passing the kit. Whatever
crosses repositories is in the contract, never in one harness's code.

### Which release of a harness runs?

The one its adapter pins. It's installed by the harness's own installer,
fetched from its repository at that release's commit and run only if its
SHA-256 matches, the way the storage module pins rclone. The installer is
the harness's to trust, but the one that runs is always the one Link
checked. Moving to a new release is a Link release.

### Where is a harness installed?

Each harness has a private runtime home,
`~/.lnk/data/agents/<agent>/harnesses/<name>`, used as its `HOME`.
The adapter selects its installation layout: Hermes, for example,
publishes a protected program payload separately from writable runtime
data. The installer and configuration run under Link's sandbox policy. Left
alone on a Mac, a harness writes `~/.openclaw`, `~/.local/bin` and a
LaunchAgent of its own. `lnk agent harness show` lists the home and any
traces outside it, and `lnk agent harness delete --harness <harness>` deletes
both. Shared files live in the agent's `home`.

### Does every start configure the harness again?

Only when something changed. Each harness CLI run is a Node or Python
start, and a full `configure` is ten or more of them.

#### `.lnk/data/harnesses/configured`

OpenClaw's and Hermes's adapters keep a hash in the home,
`.lnk/data/harnesses/configured`. It covers the settings they were given (OpenClaw's,
those its `configure` reads: not the specification, what Link knows of the model,
or the environment, so a teammate added doesn't configure it again), the
adapter's and the harness's releases, the adapter program itself (a
rebuilt one keeps its version), the agent's ports, and the settings file and
`.env` they wrote.

#### When it's configured again

When it all matches, `configure` does nothing. A harness that edits its
own settings, or a home that moved without its `.env`, is configured
again, so Link's settings still win at the next start. OpenClaw's
`doctor --fix` runs once per install of a release, when it succeeds.

### How does a harness find its tools?

In its own MCP settings: each tool the agent may call, and its memory,
is handed to the adapter by name and address on the harness's proxy, and
the adapter adds it the way that harness takes an MCP server over HTTP.
OpenClaw's `mcp.servers` and Hermes's `mcp_servers` then list it with
the servers you added, and a harness's own tools call it as any other.
An adapter removes only the servers it added, recorded in its home, so
one you added under another name stays. Link Harness reads the same
addresses from its environment.
