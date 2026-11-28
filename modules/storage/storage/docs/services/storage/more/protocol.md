# Interface

What other programs can rely on from `lnk storage`.

## `lnk storage`

What scripts read of the clouds and the files in them, each as JSON.

### `lnk storage list --json`

`[{"name", "kind", "backend", "region", "buckets": [{"resource",
"bucket", "encrypted", "remote", "files", "bytes", "owner"}]}]`, one
entry per connected cloud, with each bucket `clouds.toml` keeps for it,
an owner's whether or not the module that keeps the owner is here. A
folder without a bucket there is left out. It reads only Link's
settings and asks no other module. `backend` is an
rclone cloud's storage type, in rclone's word for it. Read by scripts
only.

#### Each bucket

`resource` is the bucket's ID, `bucket:<id>`, minted when Link added
the bucket here, or kept from where it was when an owner's folder moved
here.
`bucket` is its name in the cloud.

#### Each bucket's owner

`owner` is null for your own files,
else the folder's owner by its ID and name: `{"group": "group:<id>",
"name"}` for a hosted team's folder, and `{"workload": "<kind>:<id>",
"name"}` for what runs, such as an agent or a job. The settings keep no
owner's name, so `name` is the one the owner's module gives when asked
which folders it keeps, such as `main`. An owner whose module isn't here,
or doesn't answer, is named by its ID.

#### Bucket IDs

Each bucket's ID is reserved in `~/.lnk/safe/storage/ids.toml`. A cloud
disconnected leaves its buckets, and their IDs stay reserved. When Link
deletes a bucket (`lnk storage delete`), its ID is retired and never
given again. A `clouds.toml` with a bucket that has no ID is refused,
naming the entry to remove before connecting that cloud again.

### `lnk storage list --tree [<path>] --json`

`{"personal": [...], "<kind>:<id>": [...]}`, one key per folder, each
owner's by its ID, and only the folder holding `<path>` when given. Read
by scripts only.

#### Each row

`{"path", "size", "here", "clouds": [["<cloud>", "<mark>"], ...]}`,
`here` one of `here`, `changed`, `new`, `offloaded`, a mark one of
`checked`, `archived`, `unchecked`, `differs`, `missing`.

## An owner's buckets

The hidden commands the module that keeps an owner runs to carry the
owner's buckets when its folder moves, leaves or is restored. [Each
names the owner by its ID](../security.md#owners-folders), `--owner <kind>:<id>`, which the program
running it already holds, and refuses one that isn't an ID; `restore`
and `find` take a name too, since the owner may not be on this machine
yet.
Their output is for that program, so each prints the JSON below with
or without `--json`, while every command a person runs prints JSON only
with it.

### `lnk storage export --owner <kind>:<id>`

`{"password", "buckets": {"<cloud>": "<bucket>"}}`: the owner's buckets
here and their password, obscured as rclone keeps it; `null` without
any.

### `lnk storage adopt --owner <kind>:<id>`

Reads `{"password" | "plain", "buckets", "credentials"?: {"<cloud>":
{"bucket", "credential"}}}`; puts the owner's buckets in place here: its
encryption on buckets this machine knew, buckets in clouds of the same
name, and a cloud `<cloud>-<kind>-<id>` for it alone from each credential.
Nonzero when none is reachable (a wrong password).

### `lnk storage leave --owner <kind>:<id>`

The owner's folder left: its encryption and clouds only for it go, and
its buckets stay known.

### `lnk storage keys --owner <kind>:<id>` and `revoke`

`{"<cloud>": {"bucket", "credential"}}`: through [`lnk cloud`](../../../../../../cloud/cloud/docs/services/cloud/README.md#platforms), a credential reaching
only each of the owner's buckets (found by its `lnk-folder` label), any
before replaced; or those credentials gone. Read when its folder moves to a box
and back. `lnk storage delete` revokes them too, each with the IAM user
or service account holding it, before its bucket goes; a bucket whose
credential can't be revoked stays, and the command says why.

### `lnk storage restore --owner <kind>:<name> --folder <folder>`

Finds the owner's buckets in your `lnk cloud` accounts by their labels
(and yours, `lnk-owner`), asks their encryption password (a hidden
question on stderr, which needs a terminal), puts them in place here as
`adopt` does, with a credential reaching only them where a cloud makes one, and
pulls its files into `<folder>`. The name may be its ID's id instead,
which picks one when owners of two IDs have the name, and a name they
share is refused.

#### What it prints

`{"owner", "buckets": {"<cloud>": "<bucket>"}, "labels": {"<cloud>":
{"<name>": "<value>"}}}` on stdout, `owner` its ID, `labels` each
bucket's but those saying whose it is, and all else on stderr.

### `lnk storage label --owner <kind>:<id> <name>=<value>...`

`{"<cloud>": "<bucket>"}`: through `lnk cloud`, the labels added to each
of the owner's buckets (found by its `lnk-folder` label, never one
labelled as another login's), a bucket it couldn't label said on stderr
and left out.

#### What it refuses

A label not of Link's (`lnk-`), one that says whose a bucket is
(`lnk-folder`, `lnk-folder-name`, `lnk-owner`), and names or values
beyond 63 lowercase letters, digits, `-` and `_`.

### `lnk storage find [--owner <kind>:<name>]`

`[{"cloud", "bucket", "place", "owner", "name"}]`: owners' buckets in
your `lnk cloud` accounts, by their labels (and yours, `lnk-owner`),
only the named owner's with `--owner`.
