# Interface

> Agent Interfaces for Tools and Transfers

Integrate with `lnk agent` through structured command output, transfer commands, and requests from other modules. Your agent can also stop and ask you something, which a program or your chat answers. [Owner Questions](questions/README.md) says how.

## `lnk agent`

The commands other programs read, each for its JSON or its exit status.

### Module removal

Before removing the agent module, core calls `lnk-agent uninstall`.
It stops each agent running in a terminal through its foreground lock,
then removes its background services. A failed stop keeps the module
installed. Agent files and settings stay unless uninstall explicitly
deletes the `agents` part.

### `lnk agent list --json`

`{"agents": [...], "unreached": [{"resource", "name", "error", "cloud"?}],
"answered": [{"name", "cloud"}]}`: every agent, each as `status --json`
prints one: this machine's and each running box's (`lnk box run <box>
-- agent list --json --here`).

#### IDs in it

Every ID is written `<kind>:<id>` under [a field naming its axis](../../../../../../../packages/id/docs/more/libraries/types/id.md#in-output):
an agent's `workload`, `agent:<id>`, and a machine's, box's or team's
`resource`, `installation:<id>`, `box:<id>` and a team's group, `group:<id>`.

#### `unreached` and `answered`

`unreached` is the boxes it couldn't ask (`resource` the box,
`box:<installation ID>`), and each cloud account that runs agents but didn't
answer, or answered without its agents (`cloud`, `resource` null). `answered` is each cloud
account that runs agents and answered with its agents, any of them
there or none. An account `answered` doesn't name says nothing of the
agents there.

#### An agent on a box

An agent that moved to a box is listed as the box says, or, while the
box doesn't answer, as the note here says (`running` `null`, `location`
the box).

#### `--here`

`--here` asks only this machine, and lists no notes. It adds `in_clouds`,
`[{"workload", "name", "account", "home"}]`, each agent this machine moved to
a cloud account that runs agents (`account`), and `home`, the folder
here its Home is pushed from and pulled to.

#### `processes`

Each agent here also has `processes`, `{"pids", "group", "groups"}`:
the main processes it runs as now (its service's and its connections'
service's, or its start in a terminal; `[]` when it's stopped) and, on
Linux, its service's own control group
(`/sys/fs/cgroup/.../lnk-agent-<agent id>.service`, else `null`), and
each of its services' own (its connections' service's too,
`lnk-bridge-<agent id>.service`; never a bridge it shares with other
agents).

#### `measures`, `times` and `teams`

`measures` is the file its harness appends its own measures to
(`LNK_MEASURES`); `times` the file the harness module appends each of
its starts' and stops' times to (`~/.lnk/safe/agents/<name>/times.jsonl`,
`link_measure_contract::client::TIMES`: a line of OTLP JSON each,
`link.time.start_cold.own`, `link.time.start_warm.own` or
`link.time.stop.own`); and `teams` the teams whose files list it, each
`{"resource": "group:<id>"}` (`[]` in none). All are `null` for an agent that moved away.

#### Who reads it

`lnk agent list` on another machine (`--here`); `lnk agent connect
<connection>` (a bot or app another agent runs: `bots`, or from an older
`lnk`, the connection's own field); this module's answers below (`--here`,
and `in_clouds`: the Homes [`lnk storage push`](../../../../../../storage/storage/docs/services/storage/guide.md#back-up) and `pull` reach through the
account; whether a cloud account that hosts an agent answered without
it, so a secret's copy there went with it); and a browser-control client (your
agents, to post them to your account; `--here` between looks at the
boxes).

### `lnk agent show --json`

`{"workload", "name", "machine", "harness", "model", "running", "folder",
"files", "port", "bot", "slack", "discord", "signal", "connections",
"bots", "location", "processes", "measures", "times", "teams",
"wire", "spec", "beyond_defaults"}`, for the agent `--agent` names (else
the only one). It exits nonzero only when the settings can't be read.
Read by `lnk agent move` (whether the agent answers where it went), and
this module's answer to `secret.grantee`.

#### Current wire state

Status reads the current stop and pause flags without a sandbox child
command. Unreadable state stays unavailable through the owner's read-only
API. Observation changes no flag and supplies no authorization to open a wire.

#### `spec` and `beyond_defaults`

`spec` is what it may do on this machine while its harness runs it, the
agent's own grants and what the harness needs joined, as a sandbox's
specification (`{"internet": {"hosts": ["*"]}, "network": {"mode": "host"}}`).
`beyond_defaults` is, of that, what Link's defaults don't give, as the
flags `lnk agent permission remove` or `lnk agent harness-permission remove` takes it back with
(`["--files.read '~'", "--internet.tcp github.com:22", "--tools.names gh"]`). Both are `null`
for an agent that moved away.

#### The agent and its machine

Its ID, `"workload": "agent:<id>"` (`null` before it has one), and
name; this machine's `{"resource": "installation:<id>", "name"}` (from
`machine.toml`, whose own `id` stays bare; `null` if it can't be read or
written); its base port (`null`: the adapter's
own); the active harness; the model it answers with by name (its
provider's when it has no name; `null` before its first start); whether
the agent runs (in the background or in a terminal); and its files
folder, or `null` when that setting is one `lnk agent start` wouldn't
accept as it is.

#### Its bots

Its bot `{"id", "username"}` (never the token; `null` without
Telegram), its Slack app's bot `{"id", "name", "team"}` (its member id,
never a token; `null` without Slack), and its Discord bot `{"id",
"name"}` (its user id, never the token; `null` without Discord).

#### `connections` and `bots`

`connections` is the connections connected (`["discord", "slack",
"telegram"]`, whether or not the harness speaks them), and `bots` each
one's bot (`{"<connection>": {"id", "name", "team", "title"}}`, never a
token). `bot`, `slack`, `discord` and `signal` are the same for Link's
own connections, as they were before `bots`: Signal's `{"id"}` the
agent's number.

#### `location` and `wire`

Its `location` is `null`. `wire` is why its
harness's wire is closed (every wire stopped, or its own paused, and
why), else `null`; only `status` gives it, not `list`.

#### An agent that moved away

For an agent that moved away, what its note says: its ID (`null` until
the box says it), name, the machine it went to (`{"resource":
"installation:<id>", "name"}`, `resource` `null` until the box says it, and
`cloud` for a cloud account), `location` that machine's name, and
`null` for the rest.

### `lnk agent chat send [--chat <name>] --message <text> --json`

`{"thread", "answer", "held"}` (`link_agent::chat::Chatted`): the
message sent to the agent `--agent` names in thread `link:<name>`
(`link:main` unless named; a name is 1 to 64 letters, digits, `-`, `_`
and `.`, or a copy's, `main~hermes`), as a connection's bridge sends one to its harness's
`endpoint`, and its answer. With no `--message`, the message is stdin.
Read by scripts and checks.

#### `--action`

`--action <action>` sends one agent action and exits, as `-m` sends one
message. `model:get` and [`/model:get`](../../../../../harnesses/link/docs/agents/link/commands/README.md#model-commands) both send `/model:get`. It
conflicts with `--message`, and follows the same local or remote chat path.

#### `held`

`held` is the answers in that thread nobody waited for, handed over
first, oldest first, each one the bridge held cut at 64 KB with a line
saying `lnk agent chat show` has it whole. The C1 controls and the
bidi and zero-width marks in it are written as `\u` escapes.

#### Its exit status

0 only with an answer: nonzero when the agent is stopped, paused, fails
or answers nothing, or its harness has no `endpoint`; 130 when Ctrl-C
stopped waiting.

#### An agent elsewhere

For an agent on a box, it runs there (`lnk box run <box> -- agent chat
...`), the message on its stdin. For an agent a cloud account hosts, it
goes through that account's adapter (`agent-chat`), and `held` is
always empty.

### `lnk agent start --agent <agent> --yes --run.files <dir>`

Starts the agent with `<dir>` as its Home, its files folder, in its own
sandbox file, allowing what its harness asks beyond Link's defaults;
exit status 0 once it runs.

#### Run as a person would

a browser-control client runs `start`, `stop`, `connect`, `disconnect`, `backup`,
`restore`, `delete`, `move` and `use` as a person would, each read only
for its exit status and last line.

## Moving an Agent

The [Agent Contract](../../../../contract/docs/agents/more/interface/contract/README.md) owns the transfer types and hosting requests. Moving an agent between machines uses `lnk agent move` and `restore`, and most of the commands below are hidden from `--help`. The agent moves in the
[Move Stream](../../../../contract/docs/agents/more/interface/contract/move-stream.md), and an
account that hosts agents answers the commands of
[Accounts That Host Agents](../../../../contract/place/docs/agents/more/interface/contract/hosting.md).

### `lnk agent state-send`

`lnk agent state-send [--phase copy|switch] [--move <id>] [--held <file>|-]
[--seal-to <key>]` writes the move's stream of that phase on stdout.
Read by `lnk agent move`.

#### Phase `copy`

The agent may run: the manifest with the agent and its harness alone,
then `files/...` and `memory/<engine>/index.sqlite` (memory's index, as
SQLite's online backup of it, never the live file), after the parts
beside it (`memory/<engine>/index.sqlite.<part>`, a file or a folder's
files, such as Link Memory's words and vectors). A receiver puts them in
place of every part the index there had.

#### What `files/` holds

The agent's folder as it sees it: Home's files, and beside them its
`instructions/`, `skills/` and `chats/`, from its own folder,
`agent/`. A receiver puts those three in its own folder, the rest
in Home.

#### Phase `switch`

The default, refused while the agent runs: `files/...` and the index
again, but what `--held` (a file, or `-` for stdin) holds the same, then
the rest of the manifest, `harness/...` (the state `lnk-<name> state`
names) and `connections/<connection>/...`, links as links.

#### The rest of the manifest

`"model", "fallback"?, "connected", "telegram"?, "bot"?, "slack"?,
"slack_bot"?, "discord"?, "discord_bot"?, "signal"?, "sandbox"?,
"spec"?, "state", "connections", "bucket"`. `fallback` is its
fallback model (`lnk agent start --model.fallback_model`), its key with it, when it
has one; `sandbox` what you granted the agent (its sandbox file, a
sandbox's specification, when it has one); `spec` its own specification's text; and
`bucket` what `lnk storage export` says, or `null`.

#### `connected`

`connected` is the agent's connections as its settings keep them
(`{"<connection>": {"settings", "bot", "about"}}`), and the per-connection
fields the same for Link's own connections. A manifest without
`connected` is read from them.

#### `--seal-to`

A switch requires `--seal-to`, the key `receive` printed: the complete
tar payload is a `link_secretive::stream` envelope with HPKE context
`{"owner":"move","kind":"stream","version":1}`, 64 KiB AEAD records and
an authenticated final record.

### `lnk agent receive`

`lnk agent receive [--no-start] [--phase copy|switch]` reads one JSON
line, then `send`'s stream on stdin, into the agent's staging,
`~/.lnk/safe/agents/transfers/receiving/<agent id>` (0700). Read by `lnk agent move`.

#### Its key

A switch first prints on stdout one line, `lnk-seal-to <key>`, a key
made for it alone, whose private half it drops once it has opened what's
sealed to it.

#### Its line

`{"sealed"?, "upstream"?, "sandbox"?, "grants"?, "yes"?}`. `sealed` is
its `{"key"?, "bucket_credentials"?}` sealed to that key as `{"owner": "move",
"kind": "header", "version": 1}`: a model provider credential, and, for a box, `lnk
storage keys`' credentials. `upstream` is where its traffic goes there, and
`yes` says what its harness asks for beyond the defaults is allowed.

#### `sandbox` and `grants`

When the agent leaves the machine running the move, what it may do
there: `sandbox` the agent's sandbox file as that machine has it, kept
whole, and `grants` its harness's file (`~/.lnk/safe/agents/harness-grants/<harness>.toml`),
added to the harness's here. Both are a sandbox's specification, and an older
manifest's, with `permissions`, is read through the same table as an
older sandbox file.

#### Without `sandbox`

The agent keeps its file here, else takes the stream's without what a
stream never grants, each left out and said as `lnk agent permission add` flags:
`--files.read '~'`, `--network.mode host`, `--lan.hosts`, `--localhost.ports`, `--internet.tcp`,
`--tools.names`, `--browser.hosts`, `--commands.allow` of a path or `*`, and its teams'
folders.

#### A copy and a switch

A copy prints what staging holds then, its `have`. A switch takes out
what it says was removed and puts the agent in place here, as the agent
the stream names: its fallback the one that came (but a model on the
machine it came from that this one can't reach, left out and said), its
index in its memory engine's folder and its buckets with it (`lnk storage
adopt`).

#### Starting it

A switch starts the agent; `move` always passes `--no-start`, then asks
`lnk agent needs` and starts it with `lnk agent start`.

#### What it refuses

A switch without the complete encrypted stream envelope, sealed to
another key, tampered with, truncated or missing its authenticated
final record is refused before successful application. It refuses
a stream whose first entry isn't `lnk-move.json`, a switch of another
move than its copy, an agent here of that name with another ID, and one
of another name with its ID.

#### A switch that fails

A switch refused, or cut short, removes the staging: nothing of the
agent stays.

### `lnk agent receive --have --agent-id <agent id>`

What agent `<id>`'s staging here holds, its `have`. Read by `lnk agent
move`, before the copy.

#### Its staging

A new staging starts from the files the agent left on this machine,
when it left in the last 30 days and they're kept (hard links, or
copies across file systems). Staging no switch came for in 7 days is
removed.

### `lnk agent pause`

Hidden. Whatever carries the agent's connections stops reading them
(`~/.lnk/safe/agents/<agent>/paused`, which it says it saw with
`paused.quiet` beside it), and its way in takes nothing; a turn running
(a live lease in the conversation's store) finishes, for 60 s at most;
then the agent stops. Nothing for an agent that isn't running. Read by
`lnk agent move`, before the switch.

### `lnk agent resume`

Hidden. The pause is over, and the agent starts again; a start ends a
pause too. Read by `lnk agent move`, after a switch that failed.

### `lnk agent deliver`

Hidden. Reads JSON lines on stdin, each a message a cloud that hosts
agents held while the agent was paused there (`link_agent_place::Held`,
`{"connection", "chat", "from", "text"}`), and gives each to the agent as
its connection's bridge would. Read by `lnk agent move`, after
`agent-left`.

#### How each is given

Each is asked of its harness as a message of its owner's in that chat,
the answer sent there (`lnk-<connection> send`). Only for a connection
Link carries for it, from a sender in the connection's `allowed_users`;
any other isn't given, and the command fails, saying which.

### `lnk agent needs`

Hidden. `{"harness", "needs": [[need, why]], "running": [agent]}`: for
the agent `--agent` names, what its harness needs beyond Link's
defaults (`lnk-<harness> needs`) and hasn't been answered on this
machine, each a specification's flags (`--network.mode host`) or a risk's name,
and the agents here running that harness.

#### `--add <need>`

Repeated: grants those to the harness first, each one it asks for now,
as answered; one it doesn't ask for refuses them all. Read by `lnk agent
move` where the agent came, before it starts there: asked in the move's
terminal, granted with `--add`.

### `lnk agent left (--on <machine> | --cloud <cloud>) [--machine-id <id>]`

On the machine an agent left, once it answers on `<machine>` (with
`--cloud`, the cloud account of that name, which hosts it): deletes
its settings, harnesses, service and any staging of it, keeps its files
30 days (or `~/.lnk/safe/agents/policy.toml`'s `keep_moved`), writes its note. Read by
`lnk agent move`.

## Answers and Routes

What other modules ask of `lnk agent`, how an agent's traffic leaves its
machine, and what a move reads of a box and of [`lnk storage`](../../../../../../storage/storage/docs/services/storage/README.md#storage).

### `lnk-agent --link-answer <question>`

Hidden, and run by the asking module directly, never through `lnk`. The
questions of the modules below this one it answers, as its manifest's
`answers` lists them, [each asked and answered in its envelope](../../../../../../../core/framework/docs/more/modules/protocol.md#questions), in the
asker's types and its protocol.

#### `secret.copies`

For [`lnk secret delete`](../../../../../../security/secret/docs/config/secrets/README.md#an-agent-that-leaves): each copy of a secret a move gave, reached. On a box, it's removed there with [`lnk box run <box> -- secret delete <name> --here --service service:<id> --own <use>`](../../../../../../compute/box/docs/services/boxes/ssh.md#run-lnk-on-a-box), or gone with a box that's no longer yours; on a cloud account, gone once the account answers without the agent, else held with it.

#### `box.removed`

For [`lnk box delete`](../../../../../../compute/box/docs/services/boxes/lifecycle/delete.md#what-deletion-does): each agent this computer's note puts on the deleted box went with it, so the key made for that box to reach the agent's buckets is revoked (`lnk storage revoke`) and the note ends. A key that won't go keeps its note, and the answer says it, with the command that revokes it.

#### `secret.grantee` and `secret.token`

For [`lnk secret add --to agent:<name>`](../../../../../../security/secret/docs/config/secrets/README.md#grant-access-to-a-secret): where the agent is: `here`, by `agent:<its ID>` and its principal, `service:<id>`; `unstarted`, with the command that gives it an ID; `on` another machine; `none` by that name; or `unkept`, a kind this module doesn't keep; for the secret bridge, an agent's own token for a host.

#### `storage.folders`

For `lnk storage`, which backs up each owner's folder by the owner's ID:
this machine's agents' files folders, each owned by `agent:<its ID>`;
each one's Home on a cloud account that hosts it, with that account; and
each hosted team's folder (`~/Link/Teams/<team>/shared`), owned by its
group, `group:<its ID>`, with its account. `lnk storage` treats these
kinds as data; only this module knows what they mean.

#### `storage.home`

For `lnk storage push` and `pull`: an owned folder with its account, a
hosted agent's Home (`agent:<id>`) or a hosted team's folder
(`group:<id>`), pushed from or pulled to its folder here, through the
cloud account that hosts it (`agent-receive` and `agent-send` with phase
`files`). Only what the other end doesn't hold the same goes, and what
went and how many were the same already is the answer.

#### `storage.owner`

For [`lnk storage connect --owner <kind>:<name>`](../../../../../../storage/storage/docs/services/storage/guide.md#use-a-bucket-from-another-machine): the ID of the agent
(`agent:<name>`), or of the hosted team's group (`group:<name>`), of
that name, or whose ID's id the name is. An agent with no ID yet is
unstarted, a name none has is none, and any other kind is unkept. A
storage command this module runs itself names the agent by its ID,
`--owner agent:<id>`, and never asks.

#### `measure.agents`

For [`lnk measure`](../../../../../../tooling/measure/docs/tooling/measure/README.md#measure): the agents running here, their processes, folders
and where their harnesses report. Its existing shape remains compatible
with older readers.

#### `measure.inventory`

The agents plus `traffic` entries naming each provider and its private
snapshot directory, in the measurement contract's types. Measure prefers this
answer, falling back to `measure.agents` for an older or unavailable provider.
The sandbox source uses existing per-PID files, without taking over cleanup.

### `lnk agent route (--socks <address> | --http <address> | --direct | --none) [--default]`

Hidden. On the machine the agent runs on: its proxy's upstream, or none
for no route of its own (`direct`, unless the harness has `[network]
mode = "host"`); restarts it if running. Read by `lnk agent move` and
`lnk agent create --on` readying a box (`route --direct --default`). With `--default`, it's the route of every
agent made here, and of one a move brings here without a route.

#### Its upstreams

`--direct`; `--socks` with `socks5://127.0.0.1:<p>` or
`socks5://<socket>`; or `--http` with `127.0.0.1:<p>`, an HTTP proxy on
this machine handed each name unresolved, which it resolves. Each flag
takes only its own kind. A box's guest proxy is an HTTP one.

#### `--default`

This machine's default (`~/Link/Safe/Agents/defaults.toml`): every agent made here
from then on starts with it, and one here with no route yet takes it.

### A box's version

`lnk agent move` reads a box's `lnk --version` through [`lnk box run`](../../../../../../compute/box/docs/services/boxes/ssh.md#run-lnk-on-a-box),
its first line `lnk <version>`: a box older than this computer is
upgraded with [`lnk upgrade <version>`](../../../../../../../docs/more/modules/guide.md#upgrade).

### Buckets

A move reads the agent's buckets through `lnk storage`'s [hidden commands](../../../../../../storage/storage/docs/services/storage/more/protocol.md#an-owners-buckets),
`export`, `adopt`, `delete`, `keys`, `revoke`, `restore`, `label` and
`find`.

## The Bridge and Link's Tools

`lnk agent` runs a bridge beside an agent's harness to carry the agent's
connections and serve Local Link's tools. [Agent Bridge](bridge.md) explains the bridge.

## Understand Action Boundaries

Commands, tools, and routes run over the same actions, which [Action Namespaces](namespaces.md) defines. Caller proofs and context authority are in [Security](security.md). [Decisions](decisions.md) explains why those surfaces share one contract.

## Debug Test Controls

Debug builds accept `LNK_TEST_BRIDGE_NONE_FOR_MS` to set the empty shared
bridge's wait in milliseconds. `LNK_TEST_MIGRATE_STOP` stops a migration
before the numbered step so a test can exercise its recovery. These inputs
are test controls, not configuration for an installed release.

<!-- docs-tree:start -->

## Docs

### Pages

- [Action Namespaces](namespaces.md): An agent's actions have the same names across its surfaces: a command is `/task:run`, a tool is `task__run`, and a route is `/_link/task/run`
- [Agent Bridge](bridge.md): What `lnk agent` runs beside an agent's harness to carry its connections and serve Link's tools, as the connection contract types them
- [Owner Questions](questions/README.md): Your agent can stop and ask you something before it goes on

### More

- [Decisions](decisions.md): Why agent actions use one contract across command, tool and route surfaces
- [Security](security.md): How bridges, native context bindings and namespace actions prove callers and constrain authority

<!-- docs-tree:end -->
