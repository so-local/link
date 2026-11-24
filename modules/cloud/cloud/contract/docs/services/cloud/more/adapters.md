# Cloud Adapters

The commands a cloud adapter, `lnk-<kind>`, answers: what `lnk-aws` and
`lnk-gcp` answer, [typed](../../../../../docs/services/cloud/more/protocol.md) in `link_cloud_contract`, which `lnk-cloud`,
`lnk-box`, `lnk-storage` and `lnk-agent` call.

## Each command

How a command but `connect` is asked, what it answers, and what a box is.

### Its declaration

The package `cloud-contract` declares the protocol once: its
`actions.toml` names each command, such as `cloud-contract.cloud-contract.launch`,
and its `types.toml` the request, the reply and their records, from
which `link_cloud_contract`'s Rust is generated. An adapter lists the commands it answers under `[implements]`
and declares only its own, such as `connect`.

### Its request

Each reads one `Request` as JSON on stdin, `account` being
`lnk cloud env`'s, and prints one `Reply` as JSON on stdout
(`link_cloud_contract::{Request, Reply}`). Both are closed to keys the
declaration doesn't name, and an empty list or text is left out. A
module with commands of its own for an adapter asks them with its own
request and reply (`link_cloud_contract::ask`). Errors go to stderr with
a nonzero exit, and a caller reads `has no command` or `unrecognized
subcommand` as the command missing (`link_cloud_contract::NoCommand`).

### A box

A box is an Ubuntu LTS machine (`spec.os`) with a public address,
reachable over SSH as `ubuntu`, running `spec.user_data` (cloud-init)
at its first boot to set up the SSH certificates it trusts. An older
[`lnk box`](../../../../../../../compute/box/docs/services/boxes/README.md#boxes)
also gives a `spec.public_key`, which the box takes. The contract refuses
the machine OS type's Debian and macOS labels before cloud allocation.

## Controller contracts

Controllers preserve launch intent before allocation and transfer connected
account context through protected subprocess input, with versioned capabilities.

### Durable launch intent

A controller can set `LNK_RESOURCE_JOURNAL` to a private JSONL file. Before
`call(kind, "launch", request)` invokes the adapter, the contract locks, appends
and syncs an intent of only the adapter kind, account name, place, logical name,
generated installation ID and owner, never credentials or cloud-init. Failure to
preserve it refuses launch. A lost response keeps its intent, evidence of a
possible allocation rather than success. Each line names its schema
(`link_files::saved::line`), read by `ownership::parse_launch`.

#### Reconciliation

Match a provider's `lnk-machine` tag or label to that generated ID, with the
same account, place, owner and logical name. Preserve every preexisting native
identity and every foreign or concurrent resource; neither names nor discovery
into a local registry establish ownership. Verify removal against the native
inventory and retain unreadable inventory or failed removal as unknown.

#### Capability discovery

`lnk-box resource-journal-schema` is reserved program plumbing. With
those exact arguments it returns the typed schema version, currently 1, without
sign-in or allocation. It stays out of human help. A caller requiring the journal
checks this response before starting paid work; ordinary successful help from an
older binary does not establish support.

#### Disposable job identities

An adapter advertising `job_identities: true` also journals a self-removing VM's
service account and exact conditional IAM grant. Before any IAM mutation it
syncs `JobEvent::Intent` to `job-identities.jsonl` beside the machine journal:
account, project, zone, installation ID, nonce, email, binding and the preexisting
role, account and binding, with no credential. The account's `uniqueId` is
synced before its grant; a lost response is recovered by the nonce in its
description, never its email. Lines name their schema (`job::parse`).

#### Shared job role

Google Cloud's ordinary shared `lnkBoxEnd` role must already exist and grant
only `compute.instances.delete`. Journaled jobs refuse a missing or malformed
role and preserve its snapshot; they never create or delete that shared role.
Cleanup removes only a proven new service account and its exact conditional
member binding, preserving other members, conditions and native identities.

#### Shared firewall rules

Google Cloud's two firewall rules for boxes tagged `lnk-box`, `lnk-box-deny`
and `lnk-box-ssh`, must also exist already: journaled jobs never make or
delete them. `ownership/firewall.toml` declares them, with their actions,
sources and priorities, for the adapter and a project's setup alike
(`link_cloud_contract::ownership::firewall`).
An older capability response without the flag does not provide this guarantee.

### Connected context transfer

`lnk-cloud account-input-schema` reports version 1 for protected connected
account transfer. With exactly `account-input`, it reads a versioned
`{version, account}` on stdin and validates the account identity before saving it
through the ordinary private account/Keychain writer. It preserves every typed
profile, project, zone, credential-file selector and guarded credential; it does
not repeat sign-in with a context's defaults. Existing named accounts are never
replaced. The acknowledgement names only version, account name and adapter kind.

#### Credential boundary

The input and `cloud env` output contain credentials: transfer them only between
controlled processes, outside command capture, with no credential in arguments.
Malformed input and storage errors return a generic refusal, without rendering
the raw input or its parser errors. A caller checks the versioned capability
before sending any credential-bearing input. These reserved arguments stay out
of human menus.

## Sign in

The commands that connect an account, sign in to its buckets and forget
it.

### `lnk-<kind> connect`

`[--name] [--region] [--profile] [--key-id] [--project] [--zone]
[--credential-file] [--setting KEY=VALUE]...`: signs in to an account (the
cloud's own login, reused, else asking on the terminal, questions on
stderr), checks it, and prints its `Account` as JSON. Read by `lnk
cloud connect`.

### Its settings

Each `--setting` of [`lnk cloud connect`](../../../../../docs/services/cloud/guide.md#connect-a-cloud-account) comes as it was given, its key
lowercase letters, digits and underscores: the adapter's own settings,
kept in the account's `settings`, never a secret.

### Its sign-in

Its `env` is the account's one sign-in, which every module acts as. On
Google Cloud that is one credential file,
`GOOGLE_APPLICATION_CREDENTIALS`: a service account's key
(`--credential-file`, also `CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE`), or your
login's application default credentials.

### `lnk-<kind> storage`

`{"credential": {...}, "env"?: {...}}`: the account's own sign-in for its
buckets, as rclone's settings for them (`type` included; never saved),
and variables for the rclone run. Needs no CLI. Read by [`lnk storage`](../../../../../../../storage/storage/docs/services/storage/README.md#storage)
(each run on an aws or gcp bucket).

### What it signs in with

An aws profile (`env_auth`, `profile`) or access key; a service
account's `service_account_file`, or for a person's login `env_auth`
and `GOOGLE_APPLICATION_CREDENTIALS`. A token in `credential` may come with
rclone's own `token_url` and a `refresh_token`, so rclone refreshes it
itself during a long run.

### A sign-in that reads only

Optionally `"writes": false` and a `"message"`: the sign-in reads only
(an account past what it may keep), saying why and how to make room.
[`lnk storage push`](../../../../../../../storage/storage/docs/services/storage/guide.md#back-up) then refuses up front with that message, and `delete`
asks `delete-objects`.

### `lnk-<kind> disconnect`

`{}` once what the adapter kept for the account (Google Cloud: its
credential file) is gone. Read by [`lnk cloud disconnect`](../../../../../docs/services/cloud/guide.md#disconnect-an-account).

## Size a box

The commands that say where and how big a box is by default, and the
sizes it can be.

### `lnk-<kind> defaults`

`{"place", "size"}`: where and how big a box is by default. Read by
[`lnk box create`](../../../../../../../compute/box/docs/services/boxes/guide.md#create-a-box).

### `lnk-<kind> sizes`

`{"sizes": [{"size", "cpus", "memory_mb", "gpus", "gpu_model"?, "arch",
"price"?}], "rates"?: {"balanced_disk"?, "fast_disk"?, "address"?,
"egress"?}}`: the machine types a box can be at the request's `place`,
in the order the adapter would pick them (general purpose, current
generation, cheapest family first).

### Each size

`gpu_model` is in Link's words (`nvidia-l4`); `arch` `x86_64` or
`arm64`; `price` dollars an hour, only from a rate the cloud gives.

### Where the sizes come from

AWS lists the `describe-instance-types` the region offers
(`describe-instance-type-offerings`), Google Cloud the zone's
`machine-types list`, with the GPUs built in, and each N1 machine again
with the T4s the zone's `accelerator-types list` attaches
(`n1-standard-4.nvidia-tesla-t4.1`, a name `launch` takes as its size).

### Prices

Prices come from each cloud's price API: AWS's Price List Query API
(`aws pricing get-products`, which needs `pricing:GetProducts`),
Google's Cloud Billing Catalog API (with the account's own `gcloud`
sign-in, which needs the API on). Each is a machine type's on-demand
Linux price an hour, and the region's `rates`: a GB of a balanced or
fast disk an hour, a public address an hour and, on AWS, a GB out to
the internet.

### How long prices are kept

They are cached below `~/.lnk/safe/clouds/prices/<kind>/prices/`, with
retention read from that adapter cache's `settings.toml` (`"30m"`,
`"12h"`, or `"1d"`), a day by default. An account that can't read them lists its sizes without, and
a size no answer prices has none, never a guess.

### Who reads the sizes

Read by [`lnk box size list`](../../../../../../../compute/box/docs/services/boxes/size/README.md#list-matching-machines), and by `lnk box create` to match a specification's
CPUs, memory and GPUs or for what a pinned size has, never for the
default size; when it fails, its stderr is the error matching gives. An
adapter without it makes a pinned or default size only.

## Launch a box

The command that starts making a box from its specification, and what an adapter
makes of each of the specification's fields.

### `lnk-<kind> launch`

`{"id"}`: starts making a box from the request's `spec` (`{"name",
"id"?, "tags"?, "size", "user_data", "public_key"?, "os", "arch",
"disk_gb", "disk_type", "spot", "gpus", "gpu_model"?, "tunnel"?}`): its
machine named `lnk-<id>` (`lnk-<name>` without one) and tagged with
`tags` (`lnk-box`, `lnk-machine`, `lnk-owner`, and the specification's own). Read
by `lnk box create` and `lnk box create --image`.

### The machine's defaults

The machine fields default to the box Link made before specifications
(`ubuntu-24.04`, `x86_64`, 20, `balanced`, not spot, no GPUs), the only
box `lnk box` asks of an adapter without `sizes`, which would ignore
them.

### Each field in its cloud's words

The adapter maps each field to its cloud's: its image from a fixed
table by publisher, NVIDIA's driver with `gpus`; `balanced` gp3 or
pd-balanced, `fast` io2 or pd-ssd, at least the image's disk, saying so
on stderr when that's larger; spot a persistent request or a spot VM,
each stopping the machine when taken back. `arm64` is Ubuntu's arm64
image.

### What it refuses

It refuses what it doesn't know (`link_cloud_contract::Spec::check`),
`arm64` with GPUs among them.

### With `tunnel`

The box is reached only through `tunnel` below (`LNK_BOX_SSH=tunnel`),
so the adapter lets nothing in from outside: AWS gives it the security
group `lnk-box-tunnel`, with no inbound rule.

### With `image`

An id `image-list` gave: the box's disk starts from that image instead,
one of the account's own and ready (an AWS AMI `--owners self`, a
Google Cloud image of the project), or the launch fails.

## Run a box

The commands that wait for a box, start, stop and remove it, and say
how it is.

### `lnk-<kind> wait` and `start`

`wait` answers `{"ip"}` once the box runs, read by `lnk box create`;
`start` answers `{"ip"}` for a stopped box, running again, read by `lnk
box start <box>`.

### `lnk-<kind> stop` and `remove`

`{}` once stopped, or gone with its disk (on AWS a spot box's request
cancelled first). Read by [`lnk box stop`](../../../../../../../compute/box/docs/services/boxes/lifecycle/README.md#stop-a-box) and [`lnk box delete`](../../../../../../../compute/box/docs/services/boxes/lifecycle/delete.md#what-deletion-does).

### `lnk-<kind> state`

`{"state", "stopped_by_cloud"?}`: `running`, `stopped`, `gone`, ...,
and for a stopped box whether its cloud stopped it taking a spot
machine back rather than a person, when the adapter can tell (AWS by
the instance's state reason; Google Cloud can't). Read by `lnk box list
--refresh`.

### `lnk-<kind> describe`

`{"boxes": [{"id", "name", "machine_id", "owner"?, "place", "state",
"ip"?, "size", "stopped_by_cloud"?, "spot"?, "disk_gb"?, "disk_type"?,
"os"?, "arch"?}]}`: the box at `id` as its cloud says it is, each
machine field in `launch`'s words where the cloud says it. Read by `lnk
box spec show <box>`.

### `lnk-<kind> cleanup`

`{}`: deletes what the adapter made for boxes (a firewall rule, an
older `lnk box`'s key pairs) once its cloud says no Link box uses it;
else nothing. On AWS it
first cancels Link's spot requests whose box is gone. Read by `lnk box
delete`.

## Reach a box

The commands that pin a box's host keys, let this computer's key in
and take Link's keys out again, and carry its SSH.

### `lnk-<kind> hostkeys`

`{"host_keys"}`: the box's SSH host keys (`<type> <key>`) as
`save-hostkeys` kept them on it, else as its first boot printed them on
its console (`link_cloud_contract::host_keys` reads cloud-init's
block); none until it has. Read by `lnk box create`, and `lnk box` for
a box found in the cloud.

### `lnk-<kind> save-hostkeys`

Reads `host_keys`; `{}` once they're kept on the box's instance (Google
Cloud metadata `lnk-host-keys`, an AWS tag `lnk-host-key` with the
ed25519 key). Read by `lnk box create`.

### `lnk-<kind> add-key`

Reads `public_key`; `{"temporary"?}` once the key lets `ubuntu` in:
merged into the instance's keys (Google Cloud), or pushed with EC2
Instance Connect for 60 s (`temporary`: the caller appends it over SSH
meanwhile). Read by `lnk box`, for this computer's key on a box found
in the cloud, while the box is made to trust this computer's
certificates.

### `lnk-<kind> remove-keys`

`{}` once Link's own plain SSH keys are gone from the keys the cloud
gives the box's `ubuntu`, anyone else's kept. They're the keys whose
comment opens with `lnk`, `lnk-box` or `lnk-box-`
(`link_cloud_contract::links_ssh_key`). On Google Cloud they leave
its `ssh-keys` metadata. On AWS there's nothing to do: a key pair
reached `authorized_keys` at the first boot alone. Read by `lnk box`,
once a box takes this computer's certificates. It's newer than the
rest: an adapter without it fails, and `lnk box` warns.

### `lnk-<kind> tunnel --cloud A --zone P --id I`

No request and no reply: its stdin and stdout carry the box's SSH, its
port 22 reached over the cloud's own HTTPS, acting as the account `lnk
cloud env` names. Run by ssh, as the ProxyCommand `lnk box` gives it
when `LNK_BOX_SSH=tunnel` (`link_cloud_contract::Tunnel`).

### Each cloud's tunnel

AWS Systems Manager (`aws ssm start-session --document-name
AWS-StartSSHSession`), and Google Cloud's IAP (`gcloud compute
start-iap-tunnel <id> 22 --listen-on-stdin`).

## Find and tag

The commands that find an owner's boxes and Link's buckets, and tag
them.

### `lnk-<kind> list`

Reads `owner` and `places` (none: everywhere); `{"boxes": [{"id",
"name", "machine_id", "owner"?, "place", "state", "ip"?, "size",
"stopped_by_cloud"?}]}`, every box tagged `lnk-owner=<owner>`. Read by
[`lnk box list`](../../../../../../../compute/box/docs/services/boxes/guide.md#list-your-boxes), and `lnk box create` for a name no box of yours has.

### Every owner's boxes

With no `owner`, every box tagged `lnk-owner` at all, so a program
watching an account's boxes for many owners asks once. `owner` is each
box's `lnk-owner` tag, left out by an adapter from before it, which
refuses a request with no owner.

### `lnk-<kind> tag`

Reads `tags`; `{}` once they're added to the box's tags (labels). Read
by `lnk box list`, for a box from before tags.

### `lnk-<kind> label`

Reads `bucket` and `labels`; `{}` once they're added to the bucket's
labels (tags), the others kept. Read by `lnk storage` (an owner's new
bucket: `lnk-folder`, `lnk-folder-name`, `lnk-owner`; and `lnk storage
label`).

### `lnk-<kind> buckets`

Reads `labels`; `{"buckets": [{"bucket", "place", "labels"}]}`, Link's
buckets (`lnk-bucket-*`) with every one of them. Read by `lnk storage
find`, `keys` and `revoke`.

## Bucket credentials and objects

The commands that make and revoke a credential for one bucket, write and read
one of its objects, and delete its objects with the adapter's own
sign-in.

### `lnk-<kind> bucket-credential`

Reads `bucket`; `{"credential": {...}}`, rclone's settings (`type` included)
for a new credential reaching only that bucket's objects: an IAM user named
as the bucket, with an inline policy and a permissions boundary (AWS),
a service account
`lnk-<...>` with `roles/storage.objectAdmin` on the bucket (Google
Cloud). Its credentials from before are deleted. Read by `lnk storage keys`.

### `lnk-<kind> revoke-key`

Reads `bucket`; `{}` once that user or service account is gone (gone
already is done). Read by `lnk storage revoke`.

### `lnk-<kind> delete-objects`

Optional. Reads `bucket` and `objects`, names in the bucket (at most
1000, `link_cloud_contract::MAX_DELETE_OBJECTS`); `{}` once the
cloud deleted each (gone already is done), with a sign-in of the
adapter's own. A name outside the bucket refuses the whole call.

### Who needs it

It's for an account whose `storage` reads only; an adapter that never
says `writes: false` needs none. Read by [`lnk storage remove`](../../../../../../../storage/storage/docs/services/storage/guide.md#remove-from-your-clouds), which
falls back to rclone's delete when the adapter doesn't answer it.

### `lnk-<kind> upload-once`

Reads `bucket`, `object` (a path in it, never a flag, URL or parent
traversal, `link_cloud_contract::upload::check_object`) and
`expires` (a duration such as `30m` or `4h`, from a second to seven days); `{"upload": {"url",
"headers"}}`, an object-scoped, write-once PUT, its signed URL itself a
credential (`link_cloud_contract::upload::Upload`).

### `lnk-<kind> object-read`

Reads `bucket` and `object`; `{"contents"}`, the object's text, or none
when the bucket holds no such object.

## Images

The commands that save a stopped box's disk as an image, list an
owner's images and remove one.

### `lnk-<kind> image-save`

Reads `id` (a stopped box), `image` (its name: lowercase letters,
digits and dashes, `link_cloud_contract::check_image`) and `tags`
(`lnk-image`, `lnk-owner`, and `lnk-hash` for an image saved with a
hash); `{"images": [image]}` once the box's disk is saved as image
`lnk-<image>`, tagged with every tag given, and ready. Read by `lnk box
image create`.

### Each cloud's image

An AMI and its snapshot, both tagged (AWS, `create-image`, waited
for), or an image of its boot disk (Google Cloud, `images create
--source-disk`).

### An image

Each is `{"id", "name", "place", "state", "disk_gb"?, "arch"?,
"created"?, "hash"?}`: its id in the cloud, its `lnk-image` tag, its
region (empty on Google Cloud, where it's the project's), `ready`,
`pending` or `failed`, its disk in GiB, `x86_64` or `arm64`, when it
was made, its `lnk-hash` tag. An adapter that doesn't say `hash` still
saves the tag, and `lnk box` then finds no image there by its hash.

### `lnk-<kind> image-list`

Reads optional `owner` and `places` (none: everywhere); `{"images": [...]}`.
With `owner`, returns the account's own images tagged `lnk-image` and
`lnk-owner=<owner>`, as `image-save` prints one. Without `owner`, returns
all native images belonging to the account/project, including unlabelled
images whose logical `name` is empty. This read-only inventory does not
grant deletion authority: `image-remove` still requires the matching owner.
An adapter without it makes no image, and `lnk box` says so.

### Who reads the list

[`lnk box image list`](../../../../../../../compute/box/docs/services/boxes/image/README.md#list-images) (and `--hash`, its `hash`), `save` (a name no
image of yours has), `remove`, and `lnk box create --image` and
`--image-hash` (the newest `ready` image with that `hash`, by
`created`).

### `lnk-<kind> image-remove`

Reads `image` (an id `image-list` gave), `place` and `owner`; `{}` once
the image is gone with its snapshots (AWS `deregister-image`,
`delete-snapshot`; Google Cloud `images delete`), or was already.
Refused for an image not tagged as Link's for `owner`. Read by `lnk box
image delete`.
