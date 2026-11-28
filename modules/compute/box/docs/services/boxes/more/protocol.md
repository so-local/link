# Interface

What other programs can rely on from `lnk box`.

## `lnk box`

The commands other modules run, and what each may rely on from them.

### `lnk box forward <box> --socket <path> --proxy-port <local>`

`ssh -R` to a Unix socket at `<path>` in the box's home folder, until it
drops: a SOCKS proxy there, the one on this machine's loopback at
`<local>`.

#### Its socket

The socket's folder is made first, only the box user's, and a socket
left there by a connection before removed. `--port <p>` instead: on
`127.0.0.1:<p>` of the box, refused when the box has users of its own
(any member of `lnk-users`), whom a port there would serve too. `--to` is required: ssh's own SOCKS,
which reaches anything here, is never used.

### `lnk box run <box> -- <args>`

`lnk <args>` on the box, its input and output passed through untouched,
its exit status. Read by [`lnk agent move`](../../../../../../agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud) and [`lnk agent create --on`](../../../../../../agents/agent/docs/agents/start/create.md#create-on-a-hosting-account),
readying the box first (`module add harness secret`, `sandbox ready
--yes`, `agent route direct --default`), by `lnk agent show` (when the
agent is on a box) and by [`lnk agent list`](../../../../../../agents/agent/docs/agents/start/show.md#list-your-agents) (a box without the harness
module has no agents).

#### From `lnk secret delete`

[`lnk secret delete`](../../../../../../security/secret/docs/config/secrets/README.md#an-agent-that-leaves) runs `secret delete <NAME> --here --agent <id>` on
each box a move sent the secret to, as the move noted first, the agent's
ID that of the agent whose own key it is. Its exit status says whether
it was reached. It never runs on a box `lnk box list --json` lists with
another `resource` than the one noted.

### `lnk box create <cloud> --yes`

Makes a box in the cloud account, without asking, talking on stdout;
exit status 0 once it's up. It is then the newest (`created`) in
`account` of `lnk box list --json --here`. Read by `lnk agent move`, to
a cloud account with no box of yours, and with `--image` always.

#### `--image`

With `--image <name>`, the box's disk starts from that image of yours
there. `lnk box create <cloud>`, its name before, does the same for a
release.

### `lnk box size list --json`

The reply contains `pick`, `place`, `sizes`, `rates`, `disk_gb` and
`disk_type`. The disk fields are the effective specification's rounded allocation
and kind, so a caller can price the same disk a launch will request. Each fitting
machine type's `price` is its hourly machine price. `rates` supplies the
provider's disk, address and transfer rates, or is null when unavailable.
A missing price remains unknown. Callers calculating a complete box cost
must include its actual disk size and address rather than treating a
machine-only quote as the whole cost.

### `lnk box list --json`

`[{"name", "account", "kind", "place", "id", "resource", "size",
"ip"?, "state", "created", "found", "stopped_by_cloud"}]`,
`link_box_contract::Listed`, which the box module writes and every
reader reads: one entry per box, those this computer knows and those
each connected account has tagged as yours. `--here` asks no cloud;
`--refresh` asks each for each box's state.

#### Its fields

`created` is when this computer made it, in Unix seconds, 0 for one
found in a cloud; `found` is `"here"`, or `"cloud"` for one found just
now. `id` is the cloud's instance id, which is the cloud's and not
Link's. `resource` is the box as Link names it, `box:<its installation ID>`,
null for a box from before IDs until Link reaches it.

#### `stopped_by_cloud`

A spot box its cloud stopped, as its adapter says. From an adapter that
can't say, a spot box this computer last saw running and found stopped,
as another computer's `lnk box stop` leaves it too.

#### Who reads it

`lnk agent move` (which boxes there are, and in which `account`: a cloud
account's name moves to its one box, unasked only when its `created`
isn't 0, made on this computer) and `lnk agent list` (which to ask).

## The question it asks

What `lnk box` asks of the modules above it, in its own types.

### `box.removed`

Once `lnk box delete` has deleted a box from its cloud, it [asks each
module that answers `box.removed`](../../../../../../../core/framework/docs/more/modules/protocol.md#questions) to take back what it gave the box,
naming the box by its name and installation ID, in its envelope and its
protocol.
Each answers with what it couldn't take back, a sentence each, which
`lnk box delete` prints as a warning. The box is gone either way.

## A program that makes boxes

What a program that makes and sets up boxes may run, each read only for
its exit status but where said.

### Making and saving

`lnk box create <cloud> --name <box> --yes [--spec <file>]
[--image-hash <hash>]`; `lnk box image list <cloud>
--hash <hash> --json` (`[]` when none is ready); and `lnk box image create
<box> <image> --hash <hash> --yes`.

### Using a box

`lnk box copy <box> <files> <folder>`; `lnk box run <box> -- <args>`
(its input passed through); `lnk box ssh <box> -- <command>` (its
output passed through); `lnk box stop <box>`; `lnk box delete <box>
--yes`; and `lnk box list --here --json` (which names are boxes). On
the box, through `lnk box run`: `module add sandbox` and `sandbox ready
--yes`.
