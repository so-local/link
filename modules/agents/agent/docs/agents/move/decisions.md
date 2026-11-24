# Decisions

Why moves, cloud placement, backups and recovery preserve the agent this way.

## Moving an agent

Where an agent runs, the stream it moves in, what happens where it
lands, and how it's backed up.

### Where does an agent run?

In one place at a time, and a move carries its state, not its install.

#### Installed fresh

A harness's home on a Mac holds macOS programs that don't run on Linux,
so the machine it moves to installs the harness fresh; each adapter's
`state` says which paths are state. The harness's own Telegram bot has
one poller, so it follows the agent.

### The stream

What carries an agent between machines, how its two ends stay
compatible, and what happens to a message or a move cut short.

#### What carries an agent between machines?

One machine's `lnk agent state-send` piped into the other's `receive`, through
your computer, rather than pushed to buckets and pulled on the other
side. A box then needs no cloud keys nor the agent bucket's password to
receive, nothing sits in a bucket after, and a move is as fast as the
two SSH connections. The cost is that your computer is in the middle,
which it is anyway, since you run the move there. Backups stay on
buckets.

#### Why does a move use each place's storage contract?

A conversation kept in SQLite and one kept as files are the same
collection. Every end reads and writes them through the same collection
transfer, so a move keeps events, narrowed tools, notes and scheduled
jobs across places. Home stays a stream and an engine carries its index
in its own format.

#### How do settings reach a box?

In the move's stream, over SSH from your computer, never through the
relay, as the rest of the agent goes.

#### Why is the complete switch sealed, when SSH carries it?

SSH protects the stream on each leg, but your computer is in the middle
and passes it whole: a plaintext recording would expose linked-device
credentials and harness state as well as manifest secrets.

##### Its key pair

The receiving end makes a key pair for the switch alone. HPKE seals a
fresh streaming AEAD key to it; bounded records authenticate order and a
required final record proves completion. A machine drops its receiver
after the move.

##### A hosting service

A hosting service keeps an independent receiver for restart, deletes it
after commit when possible, and refuses it after 24 hours. The
preliminary copy retains SSH or TLS protection. It stops passive
capture; whoever holds your computer holds its SSH keys to both machines
too, which a seal never could stop.

#### How do the two ends of a move stay compatible?

The stream has a format number, and a manifest named for it
(`lnk-move.json`), so a `receive` of another format finds none of its
own and says which it found, instead of putting the agent in place of
its own. The formats must match, not the versions, since a dev build has
no release. `move` upgrades a box older than itself.

#### Why does a move go in two phases?

To keep the agent running during the bulk copy. Its files and its
memory's index, the bulk of it, go first while it runs, and only what
changed since goes once it's paused. The switch duration depends on
changed state, startup and network. One protocol for every destination is less code than a
stop-and-copy for boxes beside it, and a move to a box stops being
minutes of silence. Each file carries its SHA-256, so what the other end
holds already isn't sent again: a copy tried again, or a move back to a
machine the agent left, sends only what's new.

#### What happens to a message sent during the switch?

Connections Link carries stop reading while the agent is paused.
Messages retained by the platform can be delivered where it resumes,
subject to that connection's retention and session-resume limits. A harness that reads a
connection itself isn't paused by Link: what comes there is its own, as
always.

##### On a cloud that hosts it

A cloud that hosts agents holds what comes while the agent is paused
there and hands it over once it answers where it went, where it's kept
with the agent's settings until it's given: by `lnk agent deliver`, or
by its bridge as it reaches its harness, so an agent moved with
`--no-start` gets them when it starts.

##### Until it's given each

Until where it went has them, the computer running the move keeps them
too (`~/.lnk/safe/agents/transfers/held/`). One it's never given there, from a sender its
connection isn't paired with or on a connection Link doesn't carry for
it, is shown whole to whoever moved it rather than kept forever. The
retained delivery preserves the original message instead of replacing it
with a notice that the agent is moving.

#### What if a move is stopped midway?

From the pause until the agent answers where it went, Ctrl-C, a closed
terminal or a stop doesn't end `lnk` midway: what it runs then ends, the
question it asks is answered no, and the move fails as any failure does.
A move that ends with the agent not answering where it went fails too,
and its copy there is removed.

##### Where it answers

A move stopped midway, or one whose agent doesn't answer where it went,
leaves the agent answering where it was: resumed on a machine, the move
ended (`agent-abort`) on a cloud that hosts it, so a move tried again
isn't blocked. What its harness needs where it's going is asked while
it's paused, since it's the copy there that's asked, which exists only
after the switch.

##### Finished later

A move that can't be ended at once is kept in its journal on the
computer running it, one entry a move, finished by the next `lnk agent
move` or `lnk agent start` of the agent there. A copy stopped on a box
that doesn't answer stays an entry when the agent moves again, so it's
still removed once the box answers. An end on a cloud that hosts agents
counts as over only when its `agent-abort` says so: one that fails
keeps the entry, rather than lose track of a copy there.

#### How does a move reach a cloud with no box?

Through the cloud's adapter, which may host agents by answering the
`agent-*` commands, and owns its transport: its service's API, signed in
as the account. `lnk` only pipes the move's streams to and from it, as
it pipes a box's over SSH. The core then speaks no cloud's API and
names no service, as for boxes.

### Where it lands

When a move is done, what it replaces, and whose word it takes for what
the agent may do.

#### When is a move done?

When the agent answers where it went, running, with its ID (its `status
--json` there). Link can't see a Telegram reply, so that's the proof it
has; the files left behind stay 30 days in case it's wrong. How long is
an opinion, disk against safety: `keep_moved` in the machine's
`~/.lnk/safe/agents/policy.toml` (`"90d"`, or `"never"` to delete them yourself) changes
it for that machine.

#### What happens to the copy an agent leaves?

Its settings, state and keys are deleted once it answers elsewhere, and
its files are kept a while. A second copy would be a second truth, and a
second poller of its bot. A note says where it went, and `start` refuses
its ID.

#### What does a move replace?

The state wholly, the files folder additively. The harness's state is
its own, so the stale copy where it arrives is replaced; the files
folder is shared with you, so a move overwrites files but never deletes
them.

#### Which model does an agent use after a move?

Each machine keeps a local model it can reach. A box without a GPU can't
run the Mac's, so it needs a key once (the provider's variable, or asked
for). Back on your computer, your own model is used again if it answers.

#### Whose word does a move take for what the agent may do?

The machine running `lnk agent move`'s, never the stream's. The stream
is written by the machine the agent leaves, and when that is a box, the
box would choose what the agent may do on your computer: its whole home,
its loopback, its network past the proxy, a program its connection's
module runs outside any sandbox.

##### The agent's grants

The computer running the move sends the agent's grants in the first line
when the agent leaves. An agent coming back keeps the grants it has
there. One arriving any other way gets the stream's, but for these,
which the receive names as the `lnk agent permission add` flags that
give them here:

- `--files.read '~'`, `--network.mode host` and `--lan.hosts`.
- `--localhost.ports` and `--browser.hosts`.
- Its endpoints (`--internet.tcp`) and its outside tools (`--tools.names`).
- A program of its `[commands]` named by a path or `*`
  (`--commands.allow`). A program named alone is found on this
  machine's `PATH`, so it stays.

##### The harness's grants

A harness's grants never come from a stream: what the machine running
the move asked about, and was allowed, is granted where it lands (`lnk
agent needs --add`), for what the harness asks for there; of the
harness's grants it sends, only what the harness asks for there (its
`needs` on that machine) is kept, since a grant there holds for every
agent that runs the harness, and the file here may hold more than this
agent's harness needs anywhere else.

##### The rest of the stream

What of a connection's folder comes is what the receiving machine's own
module names, and a model on the loopback comes only from the computer
running the move. The rest of the stream (the harness's state, the
files) goes where only the sandbox reaches.

#### Are hard links in a harness's folders carried?

Yes. Memory, moves and backups keep to their folders by path: they never
follow a symbolic link, but a hard link is a regular file, read and
carried like any other. Package stores such as pnpm's and uv's make hard
links in ordinary folders, so refusing files with more than one link
would break harnesses that use them. A harness can link only a file its
sandbox shows it, and on Linux the sandbox's bind mounts stop a link
across them.

## Boxes

Why a move starts a box, and how an agent's traffic leaves one.

### Why does moving to a cloud ask before starting a box?

`lnk agent move aws` says where the agent should run, and a box
is how it runs there. A box is billed, so the move hands over to
[`lnk box create`](../../../../../compute/box/docs/services/boxes/guide.md#create-a-box), which shows what it costs and asks before making it,
and `--yes` to the move answers it. With one box there that this
computer made, that's the box. With several, `move` can't know which,
so it lists them and asks for a name.

### Why does a move with `--image` always start a new box?

An image is where a new box's disk starts, so `lnk agent move aws
--image web-ready` asks for a box made from it, even where you have a
box already: one of those wasn't made from it, or was and has changed
since. To move onto a box you have, name the box. The image is the setup
a box needs before an agent comes, done once and saved, so the agent
lands on a box ready for its work.

### Why does a cloud's name take only a box this computer made?

A box found in a cloud is yours by its `lnk-owner` tag, which anyone who
can tag machines in a shared account can set. The agent carries its
keys, tokens and a new bucket key, so `lnk agent move aws` takes its one
box unasked only when this computer made it, which [`lnk box list --json`](../../../../../compute/box/docs/services/boxes/guide.md#list-your-boxes)
tells by its `created` time. A box found by its tags is asked about on a
terminal, and without one the move stops and names it: typing a box's
name is the choice the tag alone can't make. Having reached a box once
doesn't count, since `lnk agent list` reaches every box it lists.

### Who makes the ID of an agent that runs on a box?

The machine holding its state, never the computer that moved it. A
computer keeps only a note that its agent is on a box, and learns its ID
from the box, so one agent never gets two.

### Why does a box's `lnk agent list` show only its own agents?

Giving boxes keys to find each other would let a box broken into reach
your account and your other boxes. Your computer shows them all.

## Backups

Where a backup keeps the harness's state, what a backup and a restore
replace, and what removing an agent keeps. A backup and a restore are internal
commands (`lnk agent backup`, `lnk agent restore`) that help never
offers.

### Where does a backup keep the harness's state?

In one archive in the files folder, `Backups/<harness>.tar`, pushed
like any file, rather than in a third bucket per cloud. [`lnk storage`](../../../../../storage/storage/docs/services/storage/README.md#storage)
knows two folders and nothing about harnesses, and the state is the
harness's own files, which it can read anyway.

### What does a backup replace in the clouds?

The archive and every file that changed since the last backup, since the
machine running the agent holds its latest copy. A conversation grows
every turn, so keeping the old copy would leave it out of every backup
after the first. Nothing is deleted from the clouds: a file gone here
stays there.

### What does going back to a backup replace?

The harness's state wholly, and the instructions, skills and
conversations of the agent's own folder, and nothing else. What it
learned since is what went wrong, so it's gone, from its conversations
too. The files folder is yours and keeps its
files, and the keys and settings are Link's and never in a backup. The
state is unpacked aside first, so a broken archive leaves it as it was.

### What does restoring a lost agent bring back?

The agent's ID, files and its harness's state, not its keys. The model
credential and bot token are asked again, so no key sits in a bucket, even
encrypted.
