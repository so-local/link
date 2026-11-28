# Security

Understand the sandbox boundary, protected machine state and remaining limits shared by every agent.

## Sandbox

The sandbox every harness runs in, how it keeps agents apart, what you
grant, and what no sandbox means.

### One sandbox, the sandbox module's

Which sandbox runs each harness, and how it's made.

- Every harness runs in the sandbox module's sandbox (`lnk-sandbox`,
  installed with them), on by default. The harness module asks `lnk
  sandbox wrap` for each step it runs (the installer, `configure`, the
  harness), with a confinement made from the grants below.
- How the sandbox is built on each system, and what it keeps closed
  whatever a specification says (Link's settings among them), is the [sandbox's
  security](../../../../../security/sandbox/confine/docs/tooling/sandbox/wall/README.md#the-sandbox-on-macos).
- A harness's specification writes its home and the files folder, the agent's
  Home (the harness itself only, not its installer); one that keeps to
  Link's layout writes only its conversations and the folders its
  agent's sandbox file names, of Home and of the agent's own folder, and
  reads the rest of Home, its instructions and skills, and on macOS
  `/tmp/<name>*`, where a harness may keep locks.
- [It hides](#agents-kept-apart) `~/Link/Agents` but for its own agent's folders, and `~/Link/Archive`.
- Missing sandbox permissions load closed independently of recreation. Both `Config/` files are recreated when absent, without restoring grants.
- The localhost ports it may connect to are its own, the ones its
  adapter names (its gateway or API, and the ports it derives from
  it), and its local model's.

### Agents kept apart

A machine runs any number of agents, all as your user. The sandbox keeps
each out of the others:

- Every harness's specification hides `~/Link/Agents` except
  its own agent's folders, even with `--files.read '~'`, and `~/Link/Archive`,
  the files and keys of agents that left, and `~/Link/Teams` (and each
  team's folder kept elsewhere) except what the agent is granted of its
  own teams' (below).
- What an adapter says to run can't name a folder in there, and a files folder
  can't be another agent's, nor one in its own agent's folder outside
  its `home` (where Link keeps its scheduler's jobs and its memory's
  index, out of the harness's reach).
- `lnk agent harness check` tries reading what's hidden.
- Each agent, `main` too, gets its own block of ports (`LNK_PORT`), as
  many as its harness takes and one more (200 for a harness that
  doesn't say). A harness whose adapter names a port outside it is
  refused. On macOS, without
  `[internet]`, a harness may listen only on its own ports.
- An agent doesn't start while another program listens on one of its
  harness's ports on this machine's loopback: that program would be
  taken for part of the agent, such as its connection's daemon. On macOS
  that's every port of the harness, behind its proxy too, since its
  connection daemons listen on the machine's loopback there.
- On Linux behind its proxy, it's the ports the proxy serves: the others
  are in the sandbox's own network.
- A program can still take one of those ports between that check and the
  harness's bind, or while its service restarts. So a harness whose
  adapter says `proof`, as Link Harness's does, proves it holds its
  endpoint's key before Link's connections send it anything: each
  message goes on a connection of its own, which first answers a random
  challenge with an HMAC of it made with the key.
- Whatever else answers gets neither the key nor the message, its owner
  is told it couldn't answer, and the bridge's log says why; a
  connection closed unanswered, as while its harness restarts, is tried
  again for 5 seconds. An adapter without `proof` gets the key and the
  message on whatever answers its port.
- The proof shows only that the challenge reached a harness holding the
  key: the harness answers it before any other check. A program on the
  port could relay the challenge to the harness, were the harness also
  reachable at another address.
- Link Harness and DeepSeek Harness listen only on `127.0.0.1`, and so
  do the ports a sandbox's proxy serves, so a program holding the port
  leaves the harness nowhere else to be asked.
- The bridge that carries a connection's messages runs outside the
  sandbox, and reads an answer, streamed or whole, up to 16 MiB, and a
  line of a stream up to 16 MiB; past that, the answer fails.

What stays shared:

- Other agents' localhost ports, where the sandbox lets localhost
  through. A harness's gateway asks for its token. On Linux behind its
  proxy, only that gateway is served on this machine's loopback. On
  macOS, and on Linux with `--network.mode host` or without the sandbox,
  the harness's other ports are there too.
- A connection daemon that asks for nothing there lets another program
  or user of this machine send as the agent and read its messages, so
  each harness's adapter keeps its daemons off them where its harness
  can: on a unix socket in the harness's home (700), or behind a relay
  that asks for a secret the harness is given. What each adapter does,
  and what it can't, is on its own security page.
- On macOS, `/tmp/<harness>*` between agents running the same harness;
  a harness may keep its locks there. Linux gives each sandbox its own
  `/tmp`.

Each agent needs its own bot or app:

- Telegram: `lnk agent connect telegram` refuses a bot another agent
  here has, by the bot's id (the token's public part), one that an
  agent running on a box has, or one Telegram says another program reads.
- Slack: `connect slack` refuses an app another agent here has, by its
  bot's member id or either token, or one that an agent running on a
  box has.
- Discord: the same, by the bot's user id or its token.

### One harness for an owner's agents

Off by default. With `shared_harness = true` in `~/.lnk/safe/agents/policy.toml`
(Linux), one Link Harness serves all of an owner's agents on the
machine, in one sandbox:

- Each agent keeps its own proxy, with its own model provider credential, hosts,
  endpoints, tools, calls and log. Its service runs that proxy alone
  (`lnk sandbox proxy --hold`), and the shared sandbox reaches each one
  at a port of its own, inside a network of its own with no other way
  out. A local model is reached through each agent's proxy, as on a Mac.
- The shared sandbox holds every one of those agents' folders together.
- In the shared sandbox, which agent's conversations, instructions and
  settings a request touches is held by Link Harness, not by a sandbox each: a request is
  answered as the agent on whose port it came, or the one it names with
  that agent's key, and a test shows one agent's key opens no other's,
  and no agent's words reach another's folders or prompts. Link Harness
  runs no code of its own:
- `run` and `file` still run each call in a sandbox of the agent's own.
- Only an agent whose harness can share (Link Harness), sandboxed behind
  its proxy, with no endpoint of its own (`tcp`) and no grant past
  `[internet]`, shares; any other keeps its own sandbox. Two owners' agents
  never share one.
- The shared harness starts with the first agent and starts again,
  without the one that left, when an agent stops, its proxy ends, or a
  new one joins. It starts again once the turns it's answering finish,
  waiting 30 seconds at most. A turn still running then is cut short,
  and finished when the harness recovers it, as after any crash.
- Stopping to start again, the shared harness takes no new connection,
  and waits too for each connection it has to close: a message whose
  connection opened just before the stop is answered, never cut off.
- With no agent's proxy running for 20 seconds, it ends, forgetting the
  agents that left without a word.
- Two of an owner's agents starting at once configure their harness and
  join the shared one in turn, never together: each holds
  `configure.lock` in the shared harness's folder from configuring until
  it has joined.

### Permissions

Grants open the sandbox further, in two layers, each a file of Link's
settings (600, where no sandbox reaches) holding a sandbox's
[specification](../../../../../security/sandbox/docs/config/specs/sandbox.md),
and a harness runs with both joined:

- **The agent's**, what you grant it: `lnk agent permission add|deny` with a
  specification's flags (`--internet.hosts`, `--files.read`, `--network.mode`, ...), or a
  file with `lnk agent start --sandbox`, kept in
  `~/Link/Safe/Agents/<agent>/sandbox.toml`, for any harness and no
  other agent. A deny a harness's grant still holds fails, naming it.
  [Never no sandbox](#no-sandbox), the agent's own setting.
- **A harness's**, only what it said it needs (`lnk-<harness> needs`)
  and you allowed, saved in `~/.lnk/safe/agents/harness-grants/<harness>.toml`.
  It's held only while that harness runs, never given to another, and
  the same for every agent here that runs it.
- `unsandboxed` is refused for a harness, since no sandbox at all is
  never a grant.

`lnk agent harness-permission add` and `remove` change a need a harness
declares, and nothing else: a harness only ever holds what it asked
for. `lnk agent show --permissions` shows both, each marked whose, and
its route.

- **Asked, not assumed.** A harness says what it needs beyond the
  defaults, and nothing of it is granted until you answer yes, in a
  terminal, by `lnk agent harness-permission add`, or by `--yes`. What
  the agent has of its own asks nothing.
- Switching an agent to a harness whose needs open more than its old
  harness's is asked the same way, each line with what it opens. A no
  keeps it on the harness it had.
- The question names every agent here it holds for. No changes nothing.
  The answer is kept with the needs it answered, so a later start asks
  only for a new one; a need you took back isn't asked again until `lnk
  agent harness reset`.
- A need that isn't a sandbox's specification Link takes, from an adapter older
  than this Link (`permissions`, its contract's version 1) or with a key
  Link doesn't know, is refused, and the harness doesn't start.
- A risk a harness names itself (a port of its own with no credential)
  is asked the same way; one it can run without is off until a yes in a
  terminal, never `--yes`, and the harness is told which it may open.
- It's told too whether it runs behind its proxy, which on Linux keeps
  its other ports off the machine's loopback, so it names such a risk
  wherever they're there: on a Mac, and anywhere without its proxy (no
  sandbox, or `[network] mode = "host"`).
- On a move to a box, the question is asked on this computer from the
  box's answer: a need there that isn't a specification or a risk's name Link
  takes, or an agent's name that isn't one, is refused before anything
  is shown.
- **Always shown.** The question says plainly when the programs it may
  run change; each start beyond the defaults prints what, as `status` does.
- **`needs` gets no keys.** It runs outside the sandbox, so its
  settings come with the model's key and the connections' tokens left
  empty, and its environment holds only `PATH`, `HOME`, `LANG` and
  `TMPDIR`.
- **Removed with it.** `lnk agent harness delete --harness <harness>` deletes the
  harness's grants and answers, unless another agent here runs it.
- **Moved with the agent.** A move asks on the machine running it.
  Leaving it, it asks before anything moves, and of the harness's grants
  it gives, only what the harness asks for on the other machine (its
  `needs` there) is added to the harness's there: not the rest of this
  machine's file.
- Going anywhere, it asks before the agent starts where it goes, and
  the other machine grants only what its harness asks for there (`lnk
  agent needs --add`).
- It lands in its sandbox, whatever it ran with where it was: no sandbox
  is consent for one machine, so the move of an agent whose harness
  needs none is refused there. Refused, or failing to start there, its
  copy there is removed again and it starts where it was, so two never
  answer one bot.
- The agent's own grants go with it: whole when it leaves the machine
  running the move, and kept as they are when it comes back to one that
  has them.
- Otherwise they go without `--files.read '~'`, `--network.mode host`,
  `--lan.hosts`, `--localhost.ports`, `--browser.hosts`, its endpoints (`--internet.tcp`), its
  tools that run outside (`--tools.names`), its teams' shared folders
  (`[teams.<team>]`) or a program of its `[commands]` named by a path or
  `*`, each left out and said as the `lnk agent permission add` flags that give
  it back.
- A stream never writes a harness's grants.
- **Each agent's own tools are its own.** Its memory, scheduler, run
  and file are tools Link defines for it alone (`memory-<agent id>`,
  `scheduler-<agent id>`, `run-<agent id>`, `file-<agent id>`) and
  hands only to its harness: no agent's grants name one, whether from
  `allow --tools.names`, `--sandbox-file` or a move.
- **A namespace's name is Link's.** No grant takes one (`task`, `tool`,
  `file`, `agent`): its harness is handed Link's tools under them.

- **`[internet]`** (on, any host): the internet, through its [filtering
  proxy](../../../../../security/sandbox/proxy/docs/tooling/sandbox/more/proxy/README.md),
  for hosted models and connections.
- Its installer and setup steps go through a proxy of their own, which
  leaves from this machine, and the installer and `configure` reach only
  the hosts its adapter lists (`lnk-<harness> internet-hosts`: typically its
  installer, GitHub and a package registry). Either way, macOS's
  certificate checks (`trustd`), which TLS through the Security
  framework needs, as Python's `truststore` does.
- Without `[internet]`: its proxy all the same, refusing every host,
  which serves the harness's own port, carries Link's ways in and keeps
  a local model to inference; only `[network] mode = "host"` goes
  without one.
- **`[commands]`** (Link's list): what every program of the agent may
  start, its harness's as its `run`'s: past the one Link starts (and its
  `#!` interpreter), only those listed; a harness that runs more asks in
  its needs. `developer_tools` (macOS, on): read-only Xcode tools, so
  `/usr/bin/git` runs through `xcrun`.
- **`[browser]`** (off): opening an `https://` page in your browser
  on its hosts, through its proxy, a login's for example, after asking
  you for each page. The page opens with your logins, so a URL can carry
  data out: allow only a page you asked for. No LaunchServices, and
  nothing without its proxy.
- **`[files] read = ["~"]`** (off): reading, not writing, the whole
  home folder.
- **`[localhost] ports`** (none): each port named, so a server on this
  machine. Every port, `"*"`, only with `[network] mode = "host"`:
  behind its proxy every port would be other agents' proxies too.
- **`[lan] hosts`** (off): machines of your network, through its proxy.
- **`[network] mode = "host"`** (off): the network as it is, in place
  of its filtering proxy: this machine's own addresses and its local
  network too, with the [known gaps](#known-gaps). A route ignores it.

Link's tools for the agent (its memory and scheduler, `run` and `file`)
and the wire's log are on its proxy, so a sandboxed agent without one,
with `[network] mode = "host"`, has neither. Denying `[internet]` keeps
the proxy and these local services while refusing outbound hosts.

#### Hosts

No grant opens the Keychain: no sandbox reaches `securityd`. **Hosts** (`lnk agent permission add --internet.hosts <host>` or
`lnk agent permission remove --internet.hosts <host>`, the agent's,
in its sandbox file) are the only hosts its proxy lets it reach, on
every route (`lnk agent route` too). With `"*"`, any; without
`[internet]`, none; with `[network] mode = "host"`, the list does
nothing.

#### Calls

**Calls** (`lnk agent permission add --internet.per_minute <n>`) are the most its proxy
takes within a minute, of every kind; more get a 429. The limit is the
agent's own: a deny can't lift it, and a harness's needs and grants hold
none, so none raises it.

#### Installs

**Installs** don't use these: the installer and setup steps run behind a
proxy of their own, each step with its own sockets, direct from this
machine even with a route, logged as `<agent>-<harness>-setup`.

##### Their hosts

The installer and `configure` reach only the hosts the adapter lists
(`lnk-<harness> internet-hosts`); an adapter that lists none lets them reach any
host on the internet, never this machine or its network. Its other
steps, such as `link`, which runs what the harness installed in its home
(`signal-cli`), reach only what the harness may reach when it runs: your
hosts and endpoints, any host without them.

#### Steps that read its files

Some steps only read the agent's files: `state export`, which a switch
from a stopped harness runs, `instructions` and its conversations. They
reach no network and read only the harness's home, Home and the agent's
own folder. They may run the programs the harness may run, its
`[commands]`, because an adapter may read its harness's state with what
the harness installed. Hermes's launcher, for example, is a shell script
that starts Hermes's Python. Such a step never reaches more than the
harness itself.

### Nothing it writes can widen the sandbox

Where the files that set the sandbox live, out of every harness's reach.

- The rules files live in Link's settings folder, where no harness can
  edit them.
- The files folder can't hold the harnesses' homes, so no harness
  reaches another's.
- The files folder can't be a folder whose writing is a way out: the
  home folder or one holding it, a dot folder in it (`~/.config`,
  `~/.local`, `~/.ssh`), `~/Library/LaunchAgents`, Link's settings or
  `lnk`'s own folder.
- At the top of the files folder, what runs outside the sandbox (`.git`,
  `.envrc`, `.vscode`, `.idea`) [is read only to the harness](../../../../../security/sandbox/confine/docs/tooling/sandbox/wall/README.md#what-every-sandbox-keeps-closed), so it
  commits in a repository it makes inside; in its own home it's its own.
- Its sandbox file can grant them with [`write_dot_files`](settings/README.md#files-write_dot_files),
  as Link's coding agent's does so it commits. A git hook or setting it
  writes then runs as you: a known gap, until a later sandbox closes it.
- What an adapter asks for (`lnk-<name> command`, run in the home) is
  checked too: no extra folders the harness can already write, nor the
  home folder, Link's settings, `/tmp` or any team's shared folder
  (`~/Link/Teams`, or one kept elsewhere), which it reaches only as its
  `[teams.<team>]` grants; service names that are only names; no `HOME`,
  `DYLD_*` or `LD_*`.
- Its own program, and an installed module's it runs (a store driver),
  are read wherever they are, `~/.lnk/exec/bin` too, each by its own path:
  never the folder it's in, which a test asks for and is refused.
- The environment is cleared: no `SSH_AUTH_SOCK`, no tokens from your
  shell.
- On macOS, each start writes Link's `mktemp` into the harness's home
  (`~/.lnk/exec/bin`), from outside the sandbox, without following a link:
  a link in place of `.lnk` or `bin` stops the start, and one in place
  of `mktemp` is replaced, never written through.

### Its log is out of its reach

Who writes a harness's log, and where.

- launchd and systemd, or `lnk` for an agent in a terminal, append the
  harness's output, from outside the sandbox, to a log in Link's settings
  folder (`~/.lnk/safe/agents/<agent>/logs`). Were it in the harness's home,
  the harness could replace it with a link to `~/.zshrc` and have its
  output written there.
- `lnk` refuses a log that isn't a regular file.
- `lnk agent log show` shows its last 200 lines without control characters
  (no escape sequences), from where the last start began at the
  earliest. A line that reads as Link's record shows its words and IDs
  cleaned the same way, so a harness that writes one can't style the
  terminal either. `lnk` records that
  point; it isn't a marker the harness could print.
- It reads at most the last 4 MiB, and `--follow` reads what's new at most
  4 MiB at a time and shows a line without an end in 64 KiB pieces, so a
  harness printing a burst, or no newlines, can't make it hold ever more.
- The foreground supervisor catches Ctrl-C and drains output through child
  cleanup. Log readers stay open while native processes close, including
  when the stop arrived before startup; the owned child is then reaped.

### Running a command in its sandbox

`lnk agent harness run` runs a command under the same specification as the
harness: its home, the files folder, its grants, its hosts and
endpoints, and its proxy, with the model provider credential added outside as the
harness's is. It has a rules file and a proxy of its own (on a Mac, its
own port; on Linux, its own sockets folder), so it never changes the
running agent's, and its calls are logged under its own name. With the
sandbox off (`--no-sandbox`, until `--sandbox`), it warns and runs
unconfined, as the harness does, beside a proxy of its own on a free
port.

### Checking its sandbox

`lnk agent harness check` runs [`lnk sandbox check`](../../../../../security/sandbox/docs/tooling/sandbox/guide.md#sandbox-guide)'s
[probes](../../../../../security/sandbox/docs/tooling/sandbox/security.md#checking-a-sandbox)
inside the harness's real sandbox, with its home, its grants and a
`<name>-check` profile. It refuses to run with the sandbox off. For an
agent on a team, it also reads each folder of the team's it's granted,
which must work, and each other team's folder, which must not.

#### Its keys

With keys on the wire, it also looks for the model provider credential and each secret
the proxy adds in the harness's home. It opens each name there from its
folder without following a link, and reads only regular files, at most
64 MiB each, so a harness swapping in a link or a pipe can't make it
read elsewhere or hang.

#### Its tests

Tests run a fake harness whose installer tries to write outside its
home, and which tries to read `~/.ssh` and see `/run` and other
processes: with bubblewrap where the kernel allows it, and a Seatbelt
twin on a Mac.

### No sandbox

`lnk agent start --no-sandbox` turns it all off, with a warning at every
start, until `lnk agent start --sandbox` turns it back on: one setting
of the agent's, kept across restarts, reboots and every other command,
and shown in `lnk agent show`. `--no-sandbox` is the only way: `--yes`
never implies it, and no grant of a harness's or other command turns it
off or back on. A harness that can't run in any sandbox says so
(`needs`), and starting it without `--no-sandbox` is refused, saying to
add it.

#### Who turns it off

Only you or your script turn it off, from outside any sandbox: nothing
an agent runs reaches the files that decide it, and `unsandboxed =
false` in `~/.lnk/safe/agents/policy.toml` forbids it on the machine, whatever
asks; a `~/.lnk/safe/agents/policy.toml` anyone but you (or root) could have written, or
removed (it, or a folder on its way, written by others), refuses it too.

#### On a box or in a user namespace

The choice is the same on a computer, a box, and from a user namespace:
the agent is sandboxed unless its owner explicitly supplies
`--no-sandbox`. A box or job may still be the outer isolation boundary,
but it does not silently replace that agent choice. A trusted
`unsandboxed = false` in this machine's `~/.lnk/safe/agents/policy.toml` still refuses the
explicit flag.

#### Its proxy

With no sandbox, the harness keeps its proxy, on its own port of this
machine's loopback: its tools other than `run` and `file`, [which need a sandbox](../tools/use/security.md#run-and-file), memory, scheduler, model provider credential and sealed secrets,
the wire's log, [`lnk sandbox stop`](../../../../../security/sandbox/docs/tooling/sandbox/guide.md#stop-a-sandbox) and its counted usage stay. That
proxy is advice the harness may ignore, not a wall: nothing keeps the
harness to it, and it reaches what you can, Link's settings and the keys
its proxy adds among them. [A route is refused with no sandbox](../../../../../security/sandbox/proxy/docs/tooling/sandbox/more/proxy/README.md#with-no-sandbox), which
alone would keep the harness to it.

#### A background service

A background service running a harness with no sandbox runs it through
`lnk agent unsandboxed`, which reads `~/.lnk/safe/agents/policy.toml` and the agent's own
settings first, at every restart and boot: once the machine says
`unsandboxed = false`, either can't be read, or the agent's settings
don't say `sandbox = false`, the
service refuses, says so in the harness's log, and removes itself, so it
doesn't run again until `lnk agent start` starts it in its sandbox, or
`--no-sandbox` without.

#### Ubuntu's rule for user namespaces

Where [Ubuntu's rule for user namespaces](../../../../../security/sandbox/confine/docs/tooling/sandbox/wall/README.md#ubuntus-rule-for-user-namespaces) stops the sandbox, `lnk agent
start` offers to add its profile first, and never starts the harness
unsandboxed on its own.

## On your machine

What keeps the machine up for an agent, and where Link keeps its things.

### Keeping the machine up

What keeps your agent running while the machine is idle, or you've
logged out.

#### On a Mac

The background service runs the harness under `/usr/bin/caffeinate -i`,
outside the sandbox. It only holds off idle sleep while the harness
runs; `[run] let_sleep = true` turns this off.

#### On Linux

`lnk agent start` turns on lingering for your user (`loginctl
enable-linger`) so the agent keeps running after you log out, as a
server needs. Everything else of yours that runs as a systemd user
service does too. Where systemd doesn't let you, it prints the `sudo`
command instead.

### Where Link keeps an agent's things

A harness reaches only its own home, its agent's Home, and what the
agent's sandbox file grants of the agent's own folder. Everything else
Local Link keeps for an agent, its settings, grants, keys, logs,
sockets and services, is out of every harness's reach, most of it in a
[Safe](../../../../../../docs/more/reference/locations.md#safe-exec-and-data).
[Agent Files](../state/files.md) lists each file and who writes it, and
[Move Files](../state/moves.md) lists what a move or a delete leaves.

### Revoking an Agent's Configuration

Deleting an agent or completing its move removes its editable grants
before its settings file. A later agent with that name therefore cannot
inherit the old sandbox grants. Removal unlinks only those known files
through a directory handle, refusing a linked parent folder; other
agents' control files are separate.

### Deleting an Agent's Managed Folders

A confirmed `lnk agent delete` then removes the agent's own folder in
`~/Link/Safe/Agents/` and its harness homes in
`~/.lnk/data/agents/<agent>`, with their memory, conversations,
settings and keys. Its work files stay unless you add `--delete-files`.
Each folder is found by the agent's name in its catalogued location,
never another path. A linked location is refused, and a link inside is
removed itself, never followed, so nothing outside those folders goes.
An agent's name can't name a shared entry beside them, such as
`Teams/` or `defaults.toml`.

### Finishing a Delete by Hand

Once an agent's files start going, its keys and settings go whatever a
folder does. A part that can't be removed, such as one deeper than 32
folders, is left and named in a warning with its path, for you to remove
yourself.

## Gaps and limits

What the sandbox doesn't stop, and what Link supports.

### Known gaps

A harness's sandbox has the [sandbox's known
gaps](../../../../../security/sandbox/docs/tooling/sandbox/security/gaps.md#known-gaps),
and `lnk agent harness check` reports them as `open`. Which apply
depends on its grants:

- Behind its proxy (`[internet]`, the default), the host network's
  are closed. What stays: each of its own localhost ports that
  something serves, its gateway's among them, and on Linux a new
  `.envrc` in the files folder.
- With `[network] mode = "host"`, the harness has the host network's:
  every localhost port and the machine's abstract unix sockets on
  Linux; on macOS every localhost port through `::ffff:127.0.0.1`, and the Mac's own address.
- Without `[internet]`, it reaches its own localhost ports and those it's
  granted (not a local model's), and nothing else: on Linux through a
  relay into a network of its own, which nothing outside it reaches.

The sandbox stops a harness that connects to `localhost` or `127.0.0.1`,
[not one that means to get around it](../../../../../../docs/more/architecture/security.md#known-limitations).

- **Standing instructions and skills, which the agent writes.** By
  default its sandbox file lists `instructions` and `skills` under
  `[files] write`, so asked to change how it works, it changes them.
- `file`'s and `run`'s calls write there with no question asked, so a
  page, a teammate's message or a tool's answer that steers the model
  can have it write `instructions/instruction.md` or a new skill, which
  from the next message sits in the prompt of every conversation, yours
  included.
- Its standing instructions are headed as written by you or by it, and
  a skill it wrote is listed as any other. Moving `instructions` and
  `skills` under `read` keeps them yours alone, and it then writes only
  its notes, through `remember`, capped and headed.
- **Processes on macOS.** Every stop of a `run` call or a background
  command (its timeout, `task__stop`, `lnk agent stop`, a move, a delete)
  ends the process group [`lnk sandbox run`](../../../../../security/sandbox/docs/tooling/sandbox/guide.md#sandbox-guide) leads.
- On Linux the sandbox ends everything else the call started; on macOS a
  command that leaves the group (`setsid`) outlives it, still in its
  sandbox but without its proxy, writing what the call could write and
  listening on its ports. A background command whose supervisor is
  killed has no timeout until the next sweep, and a sandboxed program's
  orphans outlive its proxy.

### Limits

Which harnesses, connections and systems Link runs agents on.

- Harnesses: Link Harness, Mini, OpenClaw, Hermes Agent and DeepSeek
  Harness, which come with Link; and those from outside Link. Connections: Telegram,
  Slack, Discord and Signal: direct messages, and mentions in
  a group, from the allowed users only.
- Link Harness sends the model as much of a conversation as its window
  takes (32,768 tokens for a model Link doesn't know). Older events stay
  in the selected conversation storage, where memory can search them.
- Link Harness's tools need a model that takes tools; with one that
  doesn't, each message fails while a tool is on.
- On Link Harness, `run` runs a command to its end within its timeout,
  or, where it takes messages from others, in the background
  (`task__run` with `run_in_background`) until its own timeout or
  `task__stop`; a program that
  needs a terminal or asks a question gets none.
- On Link Harness, videos, stickers, and unsupported or excess
  attachments are named as left out. Connection bridges download and
  validate attachments before handing them to the harness. Voice notes
  use the configured transcription model when available. The
  [media guide](../../../../harnesses/link/docs/agents/harnesses/link/media/README.md)
  owns supported types, counts, sizes, and transcription limits. An
  edited platform message is not carried as a new message.
- On Link Harness, a Discord message sent while its Gateway connection
  is down is missed when Discord won't resume the session: a short cut
  is resumed, and Discord sends what came meanwhile. Signal delivers
  what came meanwhile when it reconnects.
- On Link Harness, a Signal sender who hides their number gets no
  answer.
- Signal needs a number of the agent's own.
- Each agent runs in one place at a time; a machine runs any number.
- Behind its proxy a harness reaches the internet over TCP, through
  `HTTPS_PROXY` or `HTTP_PROXY`: not UDP, and not what ignores them.
  Anything ignoring the proxy doesn't connect.
- A backup holds one copy of the harness's state, the latest: `restore`
  goes back to it, not to an older one.
- On Linux the sandbox needs bubblewrap; on Ubuntu 23.10 and newer, also
  [an AppArmor profile](../../../../../security/sandbox/confine/docs/tooling/sandbox/wall/README.md#ubuntus-rule-for-user-namespaces).
