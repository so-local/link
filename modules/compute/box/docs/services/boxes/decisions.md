# Decisions

Why [clouds and boxes](README.md) work the way they do. Why its specification
is what it is: [Specification Decisions](../../../../../../docs/config/specs/decisions.md#the-box-specification).

## Cloud accounts

Whose account a box is made in, where its connection is kept, and how
every part of Link signs in to it as one.

### Whose cloud account do boxes use?

Yours, connected once with [`lnk cloud connect`](../../../../../cloud/cloud/docs/services/cloud/guide.md#connect-a-cloud-account) for both buckets and
boxes. Local Link holds none of these cloud accounts and adds no charge
to their bills.

### Where do cloud connections live?

A `cloud` module keeps the accounts, and one adapter module per cloud
(`lnk-aws`, `lnk-gcp`) signs in to them and makes, starts, stops and
removes machines, the way harnesses are adapters. A cloud is an engine
to swap, so the cloud and box modules never know which one they're on,
and a new cloud is a new adapter. rclone is the storage engine, the
cloud's CLI the machine engine, and both act as the account's one
sign-in.

#### How other modules reach an adapter

Other modules obtain an account through the
[cloud interface](../../../../../cloud/cloud/docs/services/cloud/more/protocol.md),
then call its adapter themselves, as `lnk box` and [`lnk storage`](../../../../../storage/storage/docs/services/storage/README.md#storage) do: one way to reach an adapter, and the cloud module
keeps no list of which commands another module may run.

### How do buckets and boxes sign in to the same cloud?

The same way, as the one [`lnk cloud`](../../../../../cloud/cloud/docs/services/cloud/README.md#platforms) account. The adapter alone knows
how to sign in, and answers `storage` with rclone's settings for the
account, so [`lnk storage`](../../../../../storage/storage/docs/services/storage/README.md#storage) holds no sign-in code for AWS or Google Cloud
and a login that works for boxes works for buckets. Two ways of signing
in had meant two sets of bugs, and a service account that worked for
boxes but not buckets.

### How does a Google Cloud account sign in?

As one credential file, kept by the adapter: a service account's key, or
your login's application default credentials. gcloud can act as a
service account's key, and rclone and Google's libraries as either, but
nothing but gcloud can use gcloud's own login. So connecting with your
login signs you in once more (`gcloud auth application-default login`),
which refreshes itself, and a service account is connected by its key
(`--credential-file`), which gcloud never hands out. A token per command would
need no second sign-in, but fails any transfer over an hour.

## Making a box

What a box is, how the cloud's CLI makes it, and how it gets `lnk` and
its releases.

### What is a box?

Just another machine running `lnk`, as your computer is. Tunnels, models
and buckets work on a box by working on a headless Linux server. Only
making a box, moving an agent to it and its exit are new.

### Why is a new box `create`, and a stopped one `start`?

`create` makes a new box in the named cloud account; `start` runs an
existing box. Each command's [positional argument](../../../../../../docs/more/developing/modules/specs.md#name-a-command) has one meaning.
Account and box names remain distinct.

### How is a box made?

With the cloud's own CLI, `aws` or `gcloud`, as buckets use rclone.
Every way of signing in to the cloud works, including profiles, SSO and
gcloud's accounts, and nothing of the cloud's API is rebuilt.

#### Calls side by side

Each call starts the CLI, most of a second before it asks anything, so
calls that don't need each other run side by side: a box's image, key
and firewall, the lists behind `lnk box size list`. A wait for an AWS box
to run, stop or go asks its state every 3 seconds, and for an image
every 10, rather than with `aws ec2 wait`, which asks every 15 and so
sees a box that runs after 10 seconds at 15.

### Where does Link install a cloud's CLI?

In Link's settings folder, for you only, from the cloud's own
installer, never with administrator rights or edits to the shell's
settings. What `lnk` installs, `lnk` can find, and nothing else on the
machine changes. The installer isn't pinned or verified, as the harness
installers aren't.

### How are SSH host keys trusted?

They're read from the box's console at its first boot and pinned in
Link's own `known_hosts` before the first connection. The computer that
pinned them keeps them in the cloud, as metadata `lnk-host-keys` on
Google Cloud or a `lnk-host-key` tag on AWS, for your other computers,
since the console's first-boot output doesn't last. The cloud account is
already what's trusted.

### Why does a box start with `lnk` alone?

A box is a machine running `lnk`; what runs on it belongs to the module
that runs it locally. So a new box gets the core and your login only,
and [`lnk agent move`](../../../../../agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud) or [`lnk agent create --on`](../../../../../agents/agent/docs/agents/start/create.md#create-on-a-hosting-account) adds what an agent needs
there first: the harness module, which brings the sandbox, the bucket
module for its backups, and [`lnk sandbox check --ready --yes`](../../../../../security/sandbox/docs/tooling/sandbox/guide.md#sandbox-guide), the sandbox's own
setup, which installs bubblewrap and its AppArmor profile through the
box's `sudo`. The box module then knows no module but `accounts` and
`cloud`, and a box never carries modules its work doesn't use.

### Does `--yes` make a box from a specification not seen before?

Yes, as `--yes` runs a sandbox's specification. `lnk box create` prints
the machine, its disk, the `lnk` it gets and the price before anything
else, even with `--yes`. A box specification has no paths, programs or
account, so it can't reach anything on this computer: what it decides is
the bill, which the printed price and the cloud's quotas bound.

### How does a box install releases it can't read?

Releases may be in a bucket of yours, read with your cloud CLI's login,
or a folder on your computer: a contributor's dev build needs no public
bucket. A box has neither, and shouldn't be given your login, so `lnk
box create` reads the release here, checks each file against the
signed `SHA256SUMS` as this `lnk` would install it, and copies it to
the box over the SSH it already uses.

#### Checked again on the box

The box installs from that copy with the same `install.sh` and checks
every signature again: trust comes from the keys, never from where the
files were. The copy holds each program's archive for the box's
processor, not just the ones it installs, so a move's [`lnk mod add`](../../../../../../docs/more/modules/guide.md#add-or-remove-a-module)
there finds the harness's; it holds only the version the box was made
with.

### Why does every build read a bucket or a folder?

A release, a staging build and a dev build take the same locations, because
the location adds no trust: a file that isn't signed by a key the build
trusts is refused wherever it's stored. One rule for every build is one
fewer kind, and a release's `lnk` gains nothing it could be tricked
with: a bucket is read with the CLI and login you already have, given
only its own variables.

## Finding your boxes

How boxes are named and found again, on this computer and on another.

### How are boxes named?

With a name you type, like `aws-1`, and a random installation ID, which names
its machine in the cloud (`lnk-<id>`). Two computers never make the same
one, and the name you type isn't the cloud's. A box without its ID
gets it from the box's `machine.toml`, with its tags, the next time Link
reaches it running.

#### A new box's name

A new box's name is one no box of yours has, as this computer knows
them and as the cloud lists them where the box goes and where your
boxes there are, never every AWS region: that is a call per region,
half a minute before `create` asks anything. A box found later
elsewhere with a name taken here is listed under its name and the
start of its installation ID (`aws-1-k3v9`).

### Which boxes in an account are yours?

The ones tagged with your account's owner ID (`lnk-owner=acct-<id>`),
or, while signed out, `lnk-owner=local-<digest>` derived from your local
user and persisted machine name. The digest fits every supported cloud's
label rules without losing parts of the local identity. Creation and
discovery use the same owner. Signing in selects the account's boxes;
boxes already recorded on this computer remain available.

### How does a new computer get into your boxes?

Through the cloud account, which already controls them: the new
computer's key is added by the cloud's own means, just long enough to
have the box trust the new computer's certificate authority. On AWS,
EC2 Instance Connect lets a key in for a minute and `lnk box` appends it
over that session, since an adapter never reads another module's key.
On Google Cloud it's merged into the instance's keys, and
`remove-keys` takes it out once a certificate gets in.

### How does a box know this computer?

By short-lived SSH certificates that this computer's own authority
signs, the same on every cloud, rather than one key in every box's
`authorized_keys`. A copied key then reaches nothing an hour later, and
a certificate names one box. OpenSSH signs and checks them
(`ssh-keygen -s`, sshd's `TrustedUserCAKeys`), so Link adds no
dependency. Each computer keeps its own authority, so no secret is
shared between computers or held by a box or a cloud.

#### Why not OS Login

Google Cloud's OS Login and AWS's Instance Connect each work on one
cloud only, with that cloud's identities. A certificate works the same
on both, on a box made from an image, and through either cloud's
tunnel.

#### Why an hour

A certificate lasts an hour and is signed again after half of it, so a
connection always starts with half an hour left. A session open longer
stays open: sshd checks a certificate only when it signs in.

### Where does the list of boxes live?

In the clouds, by tags; `boxes.toml` is this computer's copy. Boxes are
found by their tags, their host keys are kept on them, and a new
computer's key is added through the account. No machine, and not the
relay, keeps the only record: lose your computer, keep your boxes.

### What happens when several `lnk box` commands run at once?

Each change is read, made and saved under a lock (an empty `*.lock`
file in `~/.lnk/safe/boxes`), held only while it's saved, not for the
whole command: a `start` takes minutes, and `lnk box list` in another
terminal shouldn't wait for it. Each command reads `boxes.toml` again
just before it changes it, and changes only its own box. A `start`
takes its name before making the box, so two at once get `aws-1` and
`aws-2`; one stopped before its cloud made the box leaves the name,
with no id, until `lnk box delete` forgets it.

## Images

Which images a box starts from, and why saving one stops it, signs it
out and refuses its secrets.

### Which images do boxes get?

Each adapter maps the OS to its image from a fixed table, by its
publisher: Canonical's SSM parameters on AWS and its `ubuntu-os-cloud`
project on Google Cloud, and for a box with GPUs AWS's Deep Learning
Base AMI and Canonical's accelerator images. Never by searching image
names, which anyone can publish under. Only Ubuntu LTS: a box assumes
the user `ubuntu`, cloud-init and apt.

### Is an image a field of the specification, or an argument?

An argument of `lnk box create`, `--image`. An image is in one account
of one cloud, and a specification makes the same box in any account and either
cloud, so a specification naming an image would make a box in one place only.
The specification says the machine; the image says where its disk starts.

### Why does a box from an image get this `lnk`, not the image's?

So every box runs the version asked for, whatever its disk started
from. An image keeps the `lnk` and modules of the box it was saved
from; a box from it would otherwise run that build's modules, its
sandbox among them, long after `lnk` moved on, and a kept image would
quietly test an old build. Replacing a stale `lnk` costs a download;
an image of this version is used as it is.

### Why is a box stopped to be saved?

So its disk is saved whole. AWS can save a running machine without a
reboot, but files being written are then saved half-written; Google
Cloud refuses a disk in use without `--force`, for the same reason.
`lnk box image create` stops the box, saves it, and starts it again,
which costs a few minutes and a new address.

### Why are credentials refused in images?

A saved credential would be copied into every box made from the image.
Box asks every installed module which secret paths it keeps and refuses
the image while any holds data. Remove credentials through their owning
module before saving. Each new box also receives its own installation ID.

### Why does Link trust cloud-init for a new box's host keys?

Ubuntu's cloud images delete their SSH host keys and make new ones at
the first boot of each new machine, and print them on the console, so a
box from an image has keys of its own, read as any new box's are. Link
pins them under the box's own name, so the saved box's are never
trusted for it; deleting the keys on the box before saving would add a
step that cloud-init already does.

### Which images can a box start from?

Only one Link made, found by its tags (`lnk-image`, `lnk-owner`), and
only the account's own: on AWS `--owners self`, so a public AMI can't
pass for one, and on Google Cloud the project's own. An adapter from
before images answers none of the image commands, so `lnk box` says so
and makes no box from one there, while it makes every other box as
before.

### Why does saving refuse a box holding secrets, not remove them?

A box saved as an image hands its disk to every box made from it, and
the box saved is still yours, in use: deleting its git credentials or
a cloud CLI's login would break it without a word. So `save` looks
where secrets are usually left, refuses, and lists them; removing them
is yours to do. A setup whose stages each run in a sandbox of their own
keeps most of them off the disk in the first place.

## Setting a box up

How a setup runs its stages, reaches the box and reuses an image of
what it made.

### Is a box's setup a file of stages, or a script?

A box specification describes only the machine and runs nothing, so a
setup is combined with it by a command, never inside the file. A setup
of your own is a script in plain `sh`, each stage a command in it, and
an environment file is the declared form, which `lnk box create --env`
runs.

### Where does a setup run?

On this computer, where the cloud account and Link's SSH key are,
driving the box with `lnk box` commands, so one script can make the box,
set it up and save it as an image.

### How do files reach a box?

`lnk box copy`, over the box's SSH as `lnk box ssh` reaches it: Link's
key, the host keys pinned for the box, the options that keep this
computer's SSH agent and forwards off it, and the shared connection.
Never through the relay, so a file is only ever between your computer
and your box.

#### One session, many files

The files' bytes go one after another on ssh's input,
so no `scp` or `sftp` is needed on either end, and up to 64 files go
in one SSH session: the box's shell takes each file's size in bytes
(`head -c`) and checks it, so a file can't run into the next. Every
file is opened and checked before any is sent; one whose size changed
since is refused, not sent cut short or grown.

### Why does installing system packages run outside the sandbox?

It needs root, which no sandbox gives. So a setup writes it as a stage
of its own, `lnk box ssh <box> -- sudo apt-get install -y ...`, where
it can be read for what it is, before the sandboxed stages.

### How does a setup reuse an image?

By a hash the script makes of what made the image, which `lnk box image
create --hash` tags it with and `lnk box create --image-hash` finds. Link
never computes the hash, since only the script knows which files make a
stage, and the cloud keeps the tag, so any of your computers finds it.

## A box's users

Who comes to a box, as which Linux user, and where their keys are kept.

### Who comes to a box, and as whom?

A box has many users. Each person who SSHes in is a Linux user of their
own, with a sandbox of their own, and `root` is one user too. The box
declares its users and their sandboxes (`lnk box user add alice --public-key
alice.pub --sandbox dev.toml`). A sandbox granted to a person is the same thing as one
granted to an agent, so a role is a sandbox, whoever holds it.

### Why a Linux user each, and not a sandbox alone?

Two people on one box are kept apart by the system as well as their
sandboxes: their homes, their processes and what they leave in shared
folders are their own, and a mistake in one sandbox's specification opens nothing
of another's. On your own computer there are no such users: people
SSHing in as you, each in a sandbox, aren't something Link does.

### Why are a user's keys root's file, not their own?

Their home is the one folder their sandbox writes, so a key in its
`.ssh` could be replaced by one without the sandbox. sshd reads a box
user's keys from root's `/etc/lnk/users/<name>/authorized_keys` instead
(a drop-in for the group `lnk-users`), whose line runs nothing but their
sandbox. Their login shell is `/bin/sh`, which reads no file of theirs
before that command, as bash, run by sshd, would read `~/.bashrc`.

## Spot boxes, GPUs and limits

How a spot box stays a box, which GPUs a Google Cloud box gets, and
what a box doesn't cap.

### How does a spot box stay a box?

By being stopped, never deleted, when the cloud takes the machine
back, so its disk, with its login and an agent's state, stays. On AWS
that takes a persistent spot request, which would make the box again
if only its machine went, so a request is cancelled with its box.

### Which GPUs does a Google Cloud box get?

Those built into its machine type (G2, A2, A3, A4), which `machine-types
list` gives with their counts, and T4s attached to an N1 machine, the
only way Google Cloud has a T4. An N1 machine and its T4s are one size
to the box module, named `n1-standard-4.nvidia-tesla-t4.1`. The box
module matches and pins sizes by name and knows no cloud, so the name
carries everything.

#### How the adapter names them

The adapter lists each N1 machine again with every
count of T4s the zone's `accelerator-types list` allows and Google
permits with that many CPUs. It splits the name back when it makes the
box, and names a box that way when Google says it has T4s, so `lnk box
spec show` describes the same box again. Only the T4 is attached: the other GPUs
Google attaches to N1 (P4, P100, V100) are older than any Link names.

### Does a box cap memory or disk on your own computer?

A cloud box has the resources of its selected cloud machine. A box made
here uses this computer and its sandbox restrictions. It reserves and
caps none of this computer's memory or disk.

## Worker identities and persistent state

Why each sandbox on an ordinary box gets a fresh user, sees its state
through a mapped view, and never gets a number used before.

### Why does each ordinary-box sandbox get a fresh user?

A sandbox running as its sudo-capable controller could ask for all the
controller's credentials after an escape. The controller and its proxy
hold authority; work receives a locked identity with fresh unprivileged
groups. Named box users and packed slot users already have that boundary.

### Why map state instead of changing its owner or adding ACLs?

A worker must read existing private files and create private files the
controller can later back up or move. Recursive chown changes shared
state, and ACLs can outlive a run or miss newly created private files.
A private idmapped view preserves controller ownership underneath and
gives only that worker the intended view. Native mount calls pin sources,
preserve nested mounts and fail on unsupported filesystems.

### Why never reuse the numeric identities?

Another process may retain an exported directory descriptor after the
worker and its mount namespace end. Reusing the UID or a mapped GID could
make a later worker able to traverse that old view. A durable monotonic
reservation prevents that while preserving ordinary Unix IPC. Allocations
stay outside automatic and delegated subordinate ranges. Already opened
file capabilities and deliberately shared data remain authorized Unix
capabilities; credentials outside the grant never enter them.
