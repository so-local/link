# Security

What a harness and its installer can reach, and what remains protected when you switch harnesses.

## Harnesses

What a harness is trusted with, and what Link Harness, an installer and
a harness from outside Link each can reach.

### Your AI, the harness

What's protected: your machine from the harness you run (Link Harness,
one of the others that come with Link, or one from outside Link). It
acts for you with a model provider credential and, once connected, a connection's bot,
and anything it reads may try to steer it. Where its files live:
[README](../../more/settings/README.md#where-things-live).

#### What it can't protect

What the sandbox can't protect is what you give the harness: the files
folder, its own state (memory, conversations), the model provider credential, which it
can spend, and the bot. It has the network, so it can send what it can
read anywhere. A harness's built-in tools (a shell) run inside the same
sandbox; tools added with [`lnk sandbox tool add`](../../../../../../security/sandbox/docs/security/sandbox/reach.md#call-a-tool-that-runs-outside) run outside it.

#### Its gaps

Gaps, in short: the [sandbox's known gaps](../../more/security.md#known-gaps), secrets in
files and unpinned installers. See [Known
limitations](../../../../../../../docs/more/architecture/security.md#known-limitations).
What every sandbox guarantees, a harness's included, is the [sandbox's
security](../../../../../../security/sandbox/docs/security/sandbox/security.md)
page; this one is what an agent adds.

### Link Harness

What Link's own harness runs, holds and reaches.

- It runs no code itself: it reads the conversation, calls the model,
  makes the model's tool calls and writes the answer. A command the
  model runs goes to Link's `run` tool, outside it, and runs in [a
  sandbox of its own](../../tools/use/security.md#run-and-file). It runs in the same
  sandbox as every harness, with the same grants.
- Its tools are the MCP servers Link runs outside the sandbox for its
  agent (`$LNK_MEMORY`, `$LNK_TOOL_<NAME>`). It reaches them only
  through its proxy, which asks you before each call you didn't allow
  by name and logs every call, which [`lnk sandbox log show`](../../../../../../security/sandbox/docs/security/sandbox/security/log/README.md#see-every-call-on-the-wire) shows. It calls only the
  tools its environment names, never a URL a model or a tool gives it.
- A tool's answer goes to the model as the tool's words, never as
  yours or as instructions. A tool that returns text written by someone
  else, like a web page or a shared file, can still try to steer the
  model. What that steering can do is bounded by the tools you allowed
  without asking.
- A long conversation's summary, kept in its log and sent at its
  window's head, is the model's own words for what left the window. What
  a message from others or a tool's answer said can carry into it, no
  longer quoted or marked as theirs. While the model saves to its notes
  before it summarizes, it may call `memory__remember` only; any other
  call is refused.
- Turn limits come from `[turns] rounds` and `calls`; Link's defaults
  are eight rounds and sixteen calls per round. Tool answer size follows
  the tool opinions. Calls past the configured round limit are refused.
- Memory and the stateless `task` and `file` namespace sessions can be
  retained across turns, keyed by agent and server. Other tool sessions
  close with the turn. Retained sessions expire under their configured
  idle and maximum-age limits, and failed sessions are discarded. Session
  reuse does not grant another agent access to them.
- A tool that fails to start is left out of the turn and, if it never
  listed its tools, of every turn for 5 minutes. One that listed them
  before is offered what it listed, and so is one that takes over 30 s
  to list them again.
- A tool still starting after 30 s is left out of that turn. Its start
  goes on for at most 3 minutes, and the turns after have its tools. One that lists more than 16
  pages, or a page twice, is offered what it listed by then.
- What the tools offer goes into every prompt, so it has a size: at
  most 128 calls from each tool, 2 KiB of each call's description (the
  rest cut), a schema of at most 16 KiB, and 128 KiB for every call
  together. A call past a limit is left out, and said so in the agent's
  log (`lnk agent log show`).
- No tool can take another's call's name. A tool added with `__` in
  its name, or starting or ending with `_`, isn't offered. A call whose name is
  longer than a provider takes is left out rather than cut, and of two
  calls whose names come out the same, only the first is offered.
- It holds no keys by default: behind its proxy it gets a placeholder,
  and the proxy adds the model's key outside the sandbox. Without the
  sandbox, or with `--network.mode host`, its `config.json` holds the
  key, mode 600, and its fallback's key too when it has one.
- With a fallback (`--model.fallback_model`), a turn's whole conversation goes to
  that hosted provider when the model on this computer is off, sends
  nothing back for 60 seconds, or can't hold the prompt. Link Harness
  then skips it for 3 minutes, its turns going to the provider too. A
  hosted model's fallback, for a busy provider, is that provider's.
- It speaks no connection, so it never holds a connection's token or a
  linked device's keys. Link's connection module (`lnk-<connection>
  bridge`) carries the messages, outside the sandbox, beside the agent.
  It passes on the allowed users' direct messages, and their messages in
  a group that mention the bot, only.
- Every connection is a connection the module opens, so nothing listens
  for a connection on your machine: Slack in Socket Mode, Discord on its
  Gateway, `signal-cli` over its stdin and stdout, never a local port.
- An answer can't mention anyone: on Slack `&`, `<` and `>` are
  escaped, so no `@connection` or disguised link; on Discord every message
  is sent allowing no mentions, so no `@everyone`.
- Signal links as a device into
  `~/.lnk/safe/agents/<agent>/connections/<connection>` (700), which no
  harness reaches: the device's keys, and `signal-cli` itself.
  `lnk agent disconnect` deletes the folder; unlink the device on the
  phone too.
- Its endpoint listens on 127.0.0.1 only, at a port of its agent's
  block, where other users of this machine can reach it too. It refuses
  any request without the key Link makes at each start, which it gets
  in `config.json` and the bridge in `bridge.json`, both mode 600,
  compared in constant time. While its `config.json` names no key, it
  refuses every request (503), never answering one unchecked.
- It proves it holds that key before the bridge sends it, on the same
  connection: it answers the bridge's random challenge with an HMAC of
  it made with the key, never the key itself. A program that took its
  port while it started or restarted can't, and gets neither the key
  nor your message.
- A thread's name is letters, digits, `-`, `_`, `.`, one `:` and at
  most one `~`, never a path, so a message can't write outside the agent's `chats/`.
  Each agent's conversations are in its own folder
  (`~/Link/Agents/<agent>/agent/chats`); a test shows one agent
  can't read or write another's.
- It keeps each message as a line of one log per conversation,
  `events.jsonl`, only ever added to, readable only by you (600, folders
  700): the text, times, model, tokens and cost. Writers take turns on
  it with a lock, a number already written is refused, and a log that's
  a link, a pipe, or holds a line past 16 MB is refused, never followed
  or read.
- Conversations are in your selected store, so backups and moves can
  carry them. Each of the agent's answers
  holds where the model's window starts: a message's number and time,
  and token counts. One that doesn't fit the messages it names is made
  anew from them.
- A chat kept compact (`[storage] conversations = "compact"`) is the
  same lines in zstd frames, `events.zst`, and those since in
  `events.jsonl`, under the same lock and the same refusals; a frame is
  read only whole, past 64 MiB never.
- A chat a store driver keeps (`sqlite`) is in the driver's file,
  `chats/events.sqlite` (600), which it makes never through a
  link; the driver is a program of Link's or one you installed (`lnk
  module add`), run inside Link Harness's sandbox, and stopped past 30
  seconds.
- Its readable copy, `conversation.md`, which memory searches, puts a
  `\` before any line of a message that would start a heading or HTML,
  so an answer can't write a turn there as yours. A message from others
  (a teammate, a schedule, a subagent, a background command) is headed
  "Others" there, not "You", says who sent it, and has each line quoted,
  as the model reads it.
- With no tools offered in a turn, an earlier tool's answer is written
  into the text the model reads, saying which tool answered, not you,
  with each line quoted.
- The files its prompt is made of (`instructions/instruction.md` and
  `memory/notes.md`, or those its specification names) are read at each
  message, at most 64 KiB each,
  only inside the agent's files: a path with `..` is refused, and one
  through a link, or that's a folder or a pipe, is left out, so the
  agent's turns go on without it.
- Its standing instructions are headed in its prompt with who wrote
  them: you, who alone write `instructions/` by default, or, where its
  sandbox file lets the agent write it, you or the agent.
- Its skills (`skills/<name>/SKILL.md`) are read the same way at each
  message, the head of each at most 64 KiB: a skill that's a link, or
  whose `SKILL.md` is, is left out, and so is a `skills/` that's a link.
  A skill's name and description go into the prompt as instructions, and
  the model reads the rest as it would your own, so add only skills you
  trust.
- A skill's scripts run only as any command does, through `run`, for a
  program the agent's sandbox file allows.
- It reads and writes a conversation only in its own folder, made 700,
  and refuses one that is a link. It never follows a link or waits on a
  pipe there, at a file of its prompt or at its `config.json`, and
  refuses a message's file over 16 MiB and a `config.json` over 1 MiB.
- It opens each folder on the way from the one before, never through a
  link: the agent's own folder, `chats/` and a conversation's,
  and each folder to a file of its prompt or a skill.
- It holds a conversation's folder open and reads, writes, renames and
  removes in it only through that handle, so a folder swapped for a link
  meanwhile (by a `run` call) is refused, or changes nothing it reads or
  writes. A test swaps each for a link.
- Every call to a tool carries the conversation's name, its hops and
  its tools in `_meta` (`link.local/thread`, `link.local/hops`,
  `link.local/tools`), as the harness contract has it: a tool from
  outside Link learns which chat a call is from.
- Each request names its agent (`Link-Agent`). It answers only the
  agent it runs, with that agent's key: a request naming another is
  refused before its key is checked or the model sees it, and another
  agent's key proves nothing. A test shows one process answering two
  agents keeps each one's key, instructions and conversations its own.
- Everything of an agent's it keeps is keyed by the agent's ID and
  owner, never its name. A request naming by name alone an agent two
  owners share is refused (409); one whose `Link-Agent-Id` and name
  disagree, too (404). An ID two owners' agents share (the same backup
  restored by both) is the agent whose key the request carries.
- A test runs one process for two owners' `main`s, with the same ID,
  instructions, model and tool server: each answers from its own
  conversation, settings and tool sessions only.
- A move carries the agent's owner. A machine signed in as another
  account refuses the agent before anything of it is put in place.
- A move's stream is another machine's, or a cloud's, writing: every
  file in it comes with its SHA-256, which the receiver checks against
  what it wrote, refusing the stream when one differs or has none, and
  one cut short (its end entry missing) is refused too.
- Only files, folders and links come, each a plain path under the folder
  its phase allows, unpacked into staging out of every agent's reach, a
  link as a link, never followed by what comes after it.
- What a cloud that hosts agents answers (agents' names and IDs, a
  move's ID, hashes, a refusal's reason and time, held messages) is
  checked before it's used or shown, its text cleaned of escape
  sequences, as is what its adapter says on stderr.
- A held message is given to the agent only from a sender its connection
  is paired with, on a connection Link carries for it, and its answer
  goes only where that connection's `send` sends: the owner's own chat,
  or a group they talk to the agent in.
- Held messages are kept until given in `held.jsonl` with the agent's
  settings (0600), and, until where it went has them, on the computer
  running the move (`~/.lnk/safe/agents/transfers/held/`, 0600). Each
  line names its schema (`"schema": 1`), as every line of a ledger Link
  keeps does.
- From a move's pause until the agent answers where it went, Ctrl-C, a
  closed terminal or a stop fails the move safely. The agent is never
  left paused with an unstarted copy elsewhere. One move of an agent runs at a time (a lock in
  `~/.lnk/safe/agents/transfers/moving/`). Staging is seeded with copies of what the agent
  left, never hard links, so nothing written to one reaches the other.
- A pull from a cloud that hosts agents puts only files in Home: a link
  that comes is left out.
- A switch that fails before the other end answers leaves nothing of
  the agent there: its staging is removed, and the agent answers where
  it was. A copy that never switches stays in staging 7 days at most.

### Exported State

The [state export](state/README.md) contains conversation content, tool arguments and replies, and media. Native execution observations are read from the trusted runtime's stored records. They are not cryptographic attestations against an owner who can change those files. Exported native metadata excludes authorizing tokens and unrestricted request metadata. Imported updates and manifests never become local execution evidence. A missing, omitted, partial, or unknown record cannot establish success.

### Portable History

Personal adapters retain full portable records in their writable harness
homes. Files are private and bound to the agent, owner and adapter, with
links and malformed captures refused. This detects accidental mixing,
not tampering by the runtime that owns that home. Historical text and
source labels confer no permission. Bounded context marks tool results as
historical data and warns that evidence may be stale.

### Installing a harness

What a harness's install runs, and where it comes from.

- Link Harness installs nothing from a third party: each agent runs its
  adapter's own program, as installed, which its sandbox reads (that
  file, not the folder it's in) and can't change.
- Any other harness, OpenClaw, Hermes and DeepSeek Harness, which Link
  comes with, or one from outside Link, is installed by its adapter, which
  runs its harness's own installer pinned (the script from the project's
  repository at a release's commit, run only if its SHA-256 is the one
  in the adapter's source), or downloads what it needs checked against a
  SHA-256 of its own.
- What that installer fetches itself may not all be pinned: the
  adapter's page says, for [OpenClaw](../../../../../../../interop/agents/harnesses/openclaw/docs/agents/harnesses/openclaw/security.md), [Hermes](../../../../../../../interop/agents/harnesses/hermes/docs/agents/harnesses/hermes/security.md) and [DeepSeek
  Harness](../../../../../../../interop/agents/harnesses/deepseek/docs/agents/harnesses/deepseek/security.md).
- It runs over HTTPS, in the home and the sandbox: it can download
  from its allowed hosts and write its declared installation locations.
  An adapter may stage a protected program payload separately from the
  agent's writable runtime home.
- A new harness release comes with a release of its adapter.

#### Installers that run their downloads

Some installers unpack a runtime and then run it, such as OpenClaw's
Node, Hermes's uv, and DeepSeek Harness's Node and npm. Such an
installer runs what it writes in the one folder it installs into. Only
the `install` step does, only in that folder, and only for an adapter
whose `needs` say `install_runs_downloads`. Link says so as it installs:
"Installing <harness> runs the programs it downloads into its own folder
while it installs."

#### Installed folders are read only

Once installed, the folder is published read only, so the running
harness never runs a program it can write. This exception is temporary,
until Link fetches and checks those runtimes itself and publishes them
read only before the install runs.

### A harness from outside Link

Where an outside harness's adapter comes from, and what it runs as.

- Its adapter, `lnk-<name>`, comes from its own repository's releases,
  signed by that repository's key, which `lnk` [pinned when you added it](../../../../../../../docs/more/modules/security.md#modules-from-outside-link).
- The adapter itself runs as you: `lnk-agent` runs its `contract`,
  `connections`, `internet-hosts`, `endpoint`, `needs`, `traces`, `written` and `state` outside
  the sandbox. Its `install`, `configure`, `link`, `command` and
  `health`, and the harness, run in the sandbox like Link Harness's.
- What it asks for is checked as for any harness: how it says to run its
  harness can't widen the sandbox, its ports must be in its agent's
  block, and it waits at most ten minutes for its harness to answer.

### One home per harness

Where each harness, its installer and its configuration live.

- Each harness, its installer and its configuration steps (`config
  set`, a harness's own repairs) run inside the sandbox with `HOME`
  set to the harness's own home, `~/.lnk/data/agents/<agent>/harnesses/<name>`
  (only yours, mode 700). None of them can spread across the machine.
- `lnk agent harness delete --harness <harness>` deletes the home and what the adapter names
  outside it. The adapter's `lnk-<name> traces` runs outside the home
  and the sandbox, and only prints a list.
- What merely carries the harness's name as a whole word is deleted
  only after asking, never with `--yes`.

### A harness's own services

A harness's own background service (a launchd agent or systemd unit it
installs for itself) would run outside the sandbox, so `lnk agent start`
stops it and keeps its plist in `~/.lnk/safe/agents/stopped-services`, at its path in your
home folder, where you can move it back from. The names come from
the adapter's `command`, which runs in the harness's home, so none may
be Link's own: every name starting `link.` or `lnk-`, such as an agent's
service or the exit's, is refused.
