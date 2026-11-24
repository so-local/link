# Security

What's protected: your cloud accounts, whose keys `lnk cloud` keeps, and
the machines `lnk box` makes in them, against anyone but you reaching
them. Neither module talks to the relay, and Local Link holds no cloud
account: everything runs in, and is billed to, yours. What they do is
[clouds and boxes](README.md). What's on a box, its install, its
workers and its users: [What's on a Box](lifecycle/on-a-box.md); an image's:
[Box Images](image/images.md).

## Limits

What Link makes boxes on, and what it needs here to reach them.

- A cloud box is made in AWS or Google Cloud only, in your own accounts and bills. A box here runs on this computer, in its sandbox or directly.
- Boxes are Ubuntu 24.04 or 22.04 LTS on x86_64 or arm64; an arm64 box
  has no GPUs.
- Boxes need the cloud's CLI (`aws`, `gcloud`) and OpenSSH on your
  computer. `lnk box create` stops before making anything when `ssh` or
  `ssh-keygen` is missing.

## Cloud accounts

Where your cloud keys are kept, how each module acts as an account, and
the adapters and CLIs that run with them.

### Keys

A cloud account's keys are kept in Link's settings folder, or the
Keychain, and reach the cloud's CLI only in its environment.

- A reused aws login (`~/.aws`) is named, not copied: the AWS CLI, and
  rclone for buckets, read it when they run.
- A Google Cloud account is one credential file, kept in
  `~/.lnk/safe/gcp/credentials/<name>.json`, mode 600 in a 700 folder:
  a service account's key (`--credential-file`), or your login's application
  default credentials, made once by `gcloud auth application-default
  login` and copied there. `lnk cloud disconnect` deletes it.
- gcloud (with a service account's key), rclone and Google's libraries
  all act as that file, so boxes and buckets are the same identity. It
  holds a refresh token or a private key: anyone who reads it acts as
  the account.
- For a result-upload URL, gcloud receives the connected service account's
  credential-file path with `--private-key-file`. The private key's value
  stays in the file. The shell cannot select a different signing identity.
- Every other module acts as an account through its adapter only, with
  the account the [cloud interface](../../../../../cloud/cloud/docs/services/cloud/more/protocol.md) provides: [`lnk storage`](../../../../../storage/storage/docs/services/storage/README.md#storage) asks the adapter's
  `storage` for its sign-in on each command, and keeps none of it.
- An access key given instead is kept in
  `~/.lnk/safe/clouds/accounts.toml`, mode 600 in a 700 folder, as
  `lnk storage` keeps its own. On a Mac its secret part is in the
  [Keychain](../../../../../../docs/more/architecture/security.md#secrets-in-the-keychain) instead.
- A key reaches the CLI only in its environment, never on a command line,
  where other local users could see it.
- The shell's own `AWS_*` and `CLOUDSDK_*` variables are removed first,
  so a profile set in the shell can't redirect a call to another
  account.
- Adapters print an account on stdout to the module that ran them. `lnk
  cloud list` never includes keys; the internal account transfer carries credentials.

### Adapters

`lnk-aws` and `lnk-gcp` run with your keys and the cloud's CLI. They're
modules like any other: installed only from Link's signed release, found
next to `lnk`, else on the PATH.

### The cloud's CLI

Link uses the CLI on your PATH, else one [`lnk cloud connect`](../../../../../cloud/cloud/docs/services/cloud/guide.md#connect-a-cloud-account) installed
when you said yes: AWS's installer from `awscli.amazonaws.com` into
`~/.lnk/safe/aws` (on Linux, its zip and `install` script; on macOS, its
package, for the current user only), or Google's archive from
`dl.google.com` into `~/.lnk/safe/gcp`.

#### How they're fetched

Both come over HTTPS only. Link doesn't pin them or check their
signatures: it trusts those sites, as their own install instructions do.
Neither needs administrator rights, and neither changes your shell's
settings.

## Reaching a box

What a box lets in, how Link knows which boxes are yours, and where
each SSH connection's credentials and checks are explained.

### What a box exposes

A default cloud box opens SSH alone, to Link's short-lived certificates.
An explicitly attached cloud identity grants its own authority; a
self-removing [Google Cloud](../../../../../../interop/cloud/gcp/docs/services/cloud/gcp.md#a-box-that-ends-itself)
job receives a scoped job identity.
Review those permissions before running work on that box.

- Only SSH, port 22, open to the whole internet, or no port at all on
  AWS through the [cloud's tunnel](ssh.md#through-the-clouds-tunnel). Key-only: Ubuntu's images refuse passwords.
- A box takes a certificate this computer's authority signed, for it
  alone, for an hour, as [Certificates](ssh.md#certificates) explains. No plain key of
  Link's is in its `authorized_keys`.
- Nothing else is open. AWS security group `lnk-box` and Google Cloud
  firewall rule `lnk-box-ssh` allow only TCP 22. Link makes them the
  first time and removes them with the last box in their region or
  project.
- Boxes go in the account's default VPC on AWS, and its default network
  on Google Cloud. That network's own rules let in every port from
  inside it, RDP and ping, so Link adds `lnk-box-deny` there, refusing
  everything else to its boxes ahead of those rules. A rule you add
  yourself at a normal priority still wins.
- AWS instances take instance metadata only with a session token
  (IMDSv2), and have no instance role. Google Cloud instances have no
  service account (`--no-service-account --no-scopes`). So the metadata
  service, which any program on the box can reach, gives no cloud
  credentials by default.
- Every agent made on a box goes through Link's proxy (`lnk agent route
  direct --default`), never straight from the box or to the cloud's
  metadata service.

### Each SSH Connection

Every connection to a box runs your OpenSSH with Link's own key, a
certificate that lasts an hour, the box's pinned host keys, and options
that hand the box nothing of this computer. [Reach a Box](ssh.md#how-ssh-connects)
explains each credential, the first connection from another computer,
the shared connection, the cloud's tunnel and your own SSH settings.

### Which boxes are yours

Link knows a box as yours by its tags, which are as trusted as the
cloud account.

- Each box's machine is named `lnk-<installation id>` and tagged with its
  name (`lnk-box`), its installation ID (`lnk-machine`) and your account's
  owner ID (`lnk-owner=acct-<id>`), or a digest of the local user and
  machine name while signed out (`lnk-owner=local-<digest>`). On AWS
  its disk has the same tags, so the bill says whose each disk is.
- `lnk box list` asks each connected account for the boxes with your
  tag, so a project shared with another Local Link user lists only yours.
- Local ownership needs no Link login. A box created while this computer
  is signed out stays signed out; its SSH access uses the cloud account
  and this computer's certificates.
- The tags are as trusted as the account: anyone who can tag instances
  there could make a machine look like yours. So [`lnk agent move
  <account>`](../../../../../agents/agent/docs/agents/move/security.md#moving-your-agent) asks before moving to a box it found by its tags, and
  takes one unasked only if this computer made it.
- What a listing brings back goes into file names and prompts only as a
  valid name and ID; anything else is skipped.

### Your own SSH settings

Every connection to a box runs your `ssh`, which reads your
`~/.ssh/config`, so a `ProxyJump` you need still works. What would hand
the box something of this computer is turned off on the command line,
which wins over that file:

- `ForwardAgent=no`: a box never gets your SSH agent, which would let
  whoever controls it sign as you to any host your keys reach.
- `ForwardX11=no`, and `PermitLocalCommand=no`, so no `LocalCommand`
  runs here.
- `ClearAllForwardings=yes`, on every connection but `lnk box forward`'s,
  so no `LocalForward`, `RemoteForward` or `DynamicForward` of yours
  opens. `lnk box forward` exists to forward one port or socket, and
  ssh can't clear the file's without clearing it too: a forward of
  yours under `Host *` opens on that connection as well.

## Boxes here

A box here is a folder of this machine, not a machine of its own, so
what isolates it is its sandbox alone. A box here has no users of its
own, no SSH server and no forwarded ports.

### Its sandbox

Its commands run under [`lnk sandbox run`](../../../../../security/sandbox/docs/security/sandbox/guide.md#sandbox-guide) with the specification captured when it
was made, which writes the box's home and opens what that specification opens.
Link's own specification opens nothing more. The specification is never read from its
source again, so editing that file changes no box. `lnk box copy` writes
only inside the home and never through a link.

### With no sandbox

With `--no-sandbox` its commands have your own access, which this
machine's agent policy (`unsandboxed = false`) forbids at creation and
again at every command.

## Spot boxes

A spot box the cloud takes back is stopped, never deleted, so its disk,
with its `lnk` login, stays and is billed, as
[Stopped by the Cloud](lifecycle/README.md#stopped-by-the-cloud) explains.

## Removing

`lnk box delete` deletes the machine and its disk, and what each module
gave the box is taken back, as [Delete a Box](lifecycle/delete.md)
explains. Snapshots, images and anything else you made there stay.

### Disconnecting

[`lnk cloud disconnect`](../../../../../cloud/cloud/docs/services/cloud/guide.md#disconnect-an-account) forgets the account and its key, and deletes
nothing in it: [Disconnect an
account](../../../../../cloud/cloud/docs/services/cloud/guide.md#disconnect-an-account).

## Gaps

What Link doesn't protect, as each is today.

- Keys are in a file, and so is the SSH authority's secret off a Mac:
  whoever reads it can sign certificates for every box this computer
  reaches.
- SSH is open to the internet.
- The first contact's host key is trusted when neither the cloud nor a
  box's console has it.
- The cloud account is trusted for tags, keys, host keys and a box's
  first boot, and for
  what `lnk box spec show` prints of a box.
- `--yes` makes a box from a specification without asking, however new the specification.
  What it makes is printed first.
- An image holds whatever else is on the box's disk: Link checks for its
  own login, agents and the usual places secrets are left in the home
  folder, not for a secret anywhere else.
- Two images of one name in a Google Cloud project shared with another
  Local Link user collide: the second save is refused.

See [Known limitations](../../../../../../docs/more/architecture/security.md#known-limitations).

## Deadlines

A deadline is installed while the instance is made. On AWS the
root-owned timer survives reboot; root can change it, so it is not a
wall against a process running as root. Saving the box as an image
leaves the timer out of the image and puts it back, at the same time,
once the box runs again. On Google Cloud the cloud
enforces deletion; no guest poweroff timer is installed because
stopping the instance clears its scheduled deletion.

## Attached resources

What a box gets of the resources its specification's `[attach]` names, which it
never makes or deletes.

### An attached identity

An attached identity is the existing service account or instance
profile the specification names. Its metadata credentials reach whatever that
identity is granted; grant only the service's resources.

### Kept at removal

Attach lookups never create networks, firewall rules, addresses,
identities or data disks. GCP removal deletes the boot disk alone;
attached GCP disks have auto-delete disabled, and attached AWS volumes
have DeleteOnTermination disabled.

### Inventory by tag

Tag-based inventory queries fail on cloud errors and do not rely on the
caller's login ownership label.
