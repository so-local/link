# Security

What moves and backups protect, which credentials they carry and what recovery may restore.

## Moving and backing up

What a move carries and checks, and what a backup and a restore keep.

### Moving your agent

`lnk agent move` carries one agent at a time between this computer and
your boxes, or between boxes.

- **What travels:** the harness's state (its memory and conversations,
  and anything it keeps in its home but its install), the files folder,
  and Link's settings for it, **including the model provider credential, the Telegram
  bot token, the Slack app's and Discord bot's tokens, and its buckets'
  encryption password**. The password is obscured, as rclone keeps it,
  which isn't encryption.
- On a harness Link carries connections for, **Signal's linked device**
  travels too: what the connection's module names as its link
  (`lnk-<connection> state`), never a program it installed.
- **Where to:** a box you name, or `here`. A cloud account's name (`lnk
  agent move aws`) takes its one box without asking only when this
  computer made it. A box found in the account by its tags alone is
  yours only as far as anyone who can tag machines there says so: the
  move asks on a terminal, and without one refuses and says to name the
  box.
- A box made elsewhere that is named like one of your cloud accounts is
  asked about the same way. With `--image`, the move always starts a new box from that
  image of yours in the account, which this computer makes; [an image
  holds no agent](../../../../../compute/box/docs/services/boxes/image/images.md#images-you-save), and no `lnk` login.
- **How:** one machine's `lnk agent state-send` pipes into the other's `lnk
  agent receive`, through your computer, over SSH with Link's key for a
  box, through [`lnk box run`](../../../../../compute/box/docs/services/boxes/ssh.md#run-lnk-on-a-box), or into a cloud account's adapter when it hosts
  agents. Never through the relay, a bucket or a command line. A key
  given for the move (the provider's variable, or asked for) travels in
  the switch's first line.
- **Sealed on the way:** the complete switch stream, including the
  harness's home, linked-device state, settings and secret values, is
  encrypted in records of at most 64 KiB with ChaCha20-Poly1305. HPKE
  seals a fresh stream key to the receiving machine's per-transfer key,
  or the hosting account's independent per-move receiver.
- Hosted receivers survive restart for at most 24 hours, with deletion
  requested after commit; a failed deletion remains bounded by that
  expiry. This computer passes the ciphertext through. Wrong recipients,
  modified or reordered records, interruption and a missing
  authenticated final record refuse the switch before activation.
- The first line's provider and bucket secrets are separately sealed to
  the same receiving machine. The preliminary copy carries files and memory's
  index over SSH or the account's TLS. Between machines it can also carry
  the adapter-selected harness state; linked connection state waits for
  the sealed switch. A link is carried as a link, and
  a folder more than 32 deep refuses the move.
- The switch starts only after the agent is paused and stopped (`lnk
  agent pause`).
- **Receiving:** the stream is unpacked into `~/.lnk/safe/agents/transfers/receiving/<agent
  id>` (700, out of every harness's reach), each file checked against
  its SHA-256, and no path leaves it:
  - an entry that isn't a plain path under the folder its phase allows
    (`..`, a leading `/`) stops the receive
  - an entry reaching out through a link unpacked before it, or a hard
    link to a file outside, stops the receive
  - a stream of another format, from an older or newer `lnk`, is refused
    before anything is put in place, and so is an agent whose name
    another agent there has, with a different ID
  - Only then is anything replaced: the state the receiving machine's
    adapter names (`lnk-<harness> state`), in the harness's home, and
    the files folder, where incoming files overwrite and nothing is
    deleted. What the stream says is state counts for nothing there, and
    nothing else that came for the home (an install, a `.env`) is put in
    place.
  - The state goes in [as a restore's does](#backups-and-restores), from folder handles, and so do the files: each
    name is moved from the folder holding it, opened without following a
    link, so a link a harness process that outlived `lnk agent stop`
    swaps in for a folder there is replaced, never written through.
  - Links are carried as links, never followed on either side: a link in
    a files folder can't pull a file from outside it into the stream,
    nor send one out when received.
  - `send` opens each name from the folder holding it without following
    a link, and reads a file from what it opened, so a link swapped in
    while it reads, by a harness process that outlived `lnk agent stop`,
    goes as a link too, and one on the way to a state path stops the
    move.
  - A link is carried as it is, pointing wherever it pointed,
    `/etc/passwd` or `../..` included: a program you run outside the
    sandbox that follows it reaches that target, on the machine it lands
    on.
  - the link rules above hold by path: a hard link is a regular file,
    sent like any other, so a hard link a harness makes in its home or
    files folder to a file it can already read is carried, by moves and
    backups alike.
  - files and folders keep their permission bits only (`0o777`): never
    setuid, setgid or sticky, whatever the stream says.
  - a linked connection's device goes to
    `~/.lnk/safe/agents/<agent>/connections/<connection>` (700), for a
    known connection only. What of it comes is what its
    module on the receiving machine names (`lnk-<connection>
    state`), never what the stream says. A path out of that folder
    refuses the whole stream, and links and anything else are dropped,
    since the module runs outside any sandbox.
  - each connection is what its module on the receiving machine says of
    it (`lnk-<connection> describe`): which fields are secret, whether it's
    linked, its title. A name that can't be a connection's, or that no
    module there describes, is left out, and the receive names it.
    Titles and hints are printed without control characters.
  - **what the agent may do comes from the machine running the move**,
    never from the stream alone, which the machine it left wrote. An
    agent leaving the computer you run `lnk agent move` on keeps its
    grants, which that computer sends in the first line. One coming back
    keeps the grants it has on the receiving machine.
  - Any other gets the stream's, but for `--files.read '~'`, `--network.mode
    host`, `--lan.hosts`, `--localhost.ports` and `--browser.hosts`, which open the
    machine itself, and its endpoints (`--internet.tcp`), tools (`--tools.names`) and
    commands named by a path or `*` (`--commands.allow`), which reach or run
    past its sandbox.
  - Nor its teams' shared folders (`[teams.<team>]`), which only this
    machine's team files and grants give: the receive names each one
    left out as the `lnk agent permission add` flags that give it (or, for a
    team's folder, `lnk agent team add`).
  - a model on the machine's own loopback (`ollama`, `local`) is taken
    from the stream only when it comes from the computer running the
    move: a box could name any local port that answers, which the
    sandbox would open to the harness as its model's.
- **The machine it left deletes its copy** once the agent answers where
  it went, running, with its ID: its settings (model provider credential, bot token),
  its harnesses' homes (state, install, `.env`) and its service go. Its
  own folder and its Home are kept in `~/.lnk/data/agents/move` (30
  days, or `keep_moved`), out of every harness's reach.
- If it doesn't answer there, the machine it left keeps its whole copy,
  stopped, keys included, until a move completes.
- If that copy can't be deleted (its keys won't leave the machine's
  secrets), the move keeps its journal, and the next `lnk agent move` or
  `lnk agent start` of it on your computer deletes it before anything
  starts, refusing to start while it can't.
- A move that fails after the switch holds the agent stopped where it
  was, rather than start it there again, while its copy where it went
  may run: that machine didn't answer, couldn't stop it, or a cloud
  account that took the switch didn't say the move is over.
- **One place at a time:** the machine it left keeps a note of where it
  went, and `lnk agent start` refuses that agent's ID there, so two
  copies can't answer the same bot. A move between two boxes updates
  this computer's note.
- **One ID, one agent:** a machine refuses an arriving agent whose ID
  another agent there has, since its services, sockets and tools are
  keyed by it.
- **A box's word for an agent's ID** is taken unless this computer's
  note says that agent is on another machine: then the move refuses,
  before stopping anything. So a box can't bring home an agent under
  another's ID, which would revoke the bucket key of the box that agent
  is on and end the note of where it is. An agent this computer never
  had is known only by what its box says.
- The stream must then be of that ID: `lnk agent receive` is told the
  agent your computer chose, and refuses a stream of any other before
  anything of it is staged.
- **A box's backups:** a box holds no cloud account of yours. For its
  agent's buckets, your computer makes a key reaching only them.
- On AWS it's an IAM user named as the bucket, under the path `/lnk/`,
  whose inline policy allows that bucket's objects and listing it,
  nothing else, and whose permissions boundary allows no more. On Google
  Cloud it's a service account with `roles/storage.objectAdmin` on that
  bucket alone.
- It sends the key in the move's first line, sealed to the box, and
  deletes the key the bucket had before. The box keeps it in its
  `rclone.conf` (600). When the agent comes back to your computer, or
  goes to a cloud account that hosts agents, the key's user or service
  account is deleted; a move to another box replaces the key.
- When you delete the box with [`lnk box delete`](../../../../../compute/box/docs/services/boxes/lifecycle/delete.md#what-deletion-does), the key is deleted with
  it, and so is your computer's note of the agent there.
- **Someone who takes over a box** gets that agent's bucket, still
  encrypted with the agent's password, which the box also holds.
- Making the key needs an account allowed to make IAM users or service
  accounts. Without one, the box has no backups, and the move says so.

### Backups and restores

What a backup writes, and what a restore puts back. `lnk agent backup`
and `lnk agent restore` are internal commands that help never offers.

- **Backups:** `lnk agent backup` writes the harness's state (the same
  paths a move sends, never its `.env` of keys, nor Link's settings) to
  `Backups/<harness>.tar` in the files folder (600).
- It reads the state as a move's `send` does, never through a symbolic
  link (a hard link is carried, as a move carries it), and writes the
  archive through the backup folder's handle: a link in the folder's
  place is removed first, and one swapped in after isn't written
  through. It then [`lnk storage push`](../../../../../storage/storage/docs/services/storage/guide.md#back-up)es it and the folder to your clouds,
  encrypted if the agent's buckets are.
- The harness's own settings files go with it, and may hold what it keeps
  there itself, such as a local gateway's token. Beside it,
  `Backups/Agent.tar` holds the agent's instructions, skills and
  conversations from its own folder; never its memory's index or its
  scheduled jobs, so nothing restored from a backup acts by itself.
- A full `lnk agent restore` of an agent not here unpacks it in a
  `~/Link/Agents/.restoring-<random>` too, and refuses it whole if it
  holds a link anywhere, symbolic or hard: it sits in Home, which the
  agent may write. Nothing of the Home that came is ever moved into its
  own folder.
- **Going back to a backup:** `lnk agent restore` of an agent that's
  here pulls `Backups/<harness>.tar` and `Backups/Agent.tar` from your
  clouds and unpacks each in a `~/Link/Agents/.restoring-<random>`, out
  of every agent's reach, the way a move's stream is unpacked, so no
  path leaves it.
- It moves only the harness's state paths into its home, never its
  install or `.env`, and only the instructions, skills and conversations
  into the agent's own folder, refusing that archive whole if it holds a
  link. What was said since the backup is gone from both.
- The archive sits in the files folder, which the agent can write, but
  it only ever lands in the harness's own home, which the agent can
  write too.
- It's opened only once the agent is stopped, and through the backup
  folder's handle: the folder and the archive are each opened without
  following a link, and the archive refused unless it's a regular file,
  so a link put in the backup folder's place, to another agent's backup,
  or a pipe, is never opened.
- A harness process that outlived `lnk agent stop` may still change that
  home meanwhile, so the restore removes and moves each name from the
  folder holding it, opened without following a link: a link on the way
  to a state path stops it, and one in a state path's place is removed
  or replaced, never followed.
- **Restoring a lost agent:** `lnk agent restore <name>` finds an
  agent's buckets by their labels in your [`lnk cloud`](../../../../../cloud/cloud/docs/services/cloud/README.md#cloud-accounts) accounts, asks for
  the encryption password with echo off, and adopts the buckets with a
  key reaching only them, as a box does. The model provider credential and bot token
  aren't in a bucket: they're asked again.
- A harness backup restores only its declared state paths, through
  `~/Link/Agents/.restoring-<random>`. Its name selects an installed
  harness: `link`, `mini`, `openclaw`, `hermes`, `deepseek`, or one you
  added from outside Link. Add an outside harness
  before restoring its state.
