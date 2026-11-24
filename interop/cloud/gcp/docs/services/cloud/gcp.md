# Google Cloud

> Google Cloud, Your Agent's Next Address

`lnk-gcp` is the Google Cloud adapter: accounts, buckets, boxes and
images on Google Cloud, through gcloud. `lnk cloud`, `lnk box` and `lnk
storage` run it by [the cloud contract](../../../../../../modules/cloud/cloud/contract/docs/services/cloud/more/adapters.md);
nobody types it. Connect the Google Cloud account you want Link to use.

```sh
lnk cloud connect gcp
```

## Adapter Interface

The Google Cloud adapter implements the requests defined by
[Cloud Adapters](../../../../../../modules/cloud/cloud/contract/docs/services/cloud/more/adapters.md).
It reads each request as a typed command line and dispatches it to
Google Cloud's resource APIs. Use the public cloud, box, and storage
commands for interactive work.

## An account

`lnk-gcp connect` connects a Google Cloud account as one credential
file that gcloud, rclone and Google's libraries all act as.

### The credential file

A service account's key (`--credential-file`), or your own login's application
default credentials (`gcloud auth application-default login`, once).
The file is kept in Link's settings (`gcp/credentials/<name>.json`,
only yours), so every part of Link using the account is the same
identity.

### Saved Identity

Both credential kinds explicitly select the saved file for gcloud too.
Previously saved accounts with only `GOOGLE_APPLICATION_CREDENTIALS`
receive the same SDK override at each call. See
[credential isolation](gcp/security.md) for its limits.

### Its flags

[`lnk cloud connect gcp`](../../../../../../modules/cloud/cloud/docs/services/cloud/guide.md#connect-a-cloud-account) passes its flags on: one type, read from flags
or a specification file alike.

### Its CLI

When boxes need gcloud and it's missing, it installs it for you from
Google's own archive at `dl.google.com`, unpacked into Link's settings
folder (`~/.lnk/safe/gcp`), for you only: no administrator rights, and its
installer runs without touching your shell's settings.

## Boxes

Each box is a Compute Engine instance of Ubuntu LTS in the default
network, named `lnk-<box id>` and labelled with its name, ID and owner.

### Its keys

It's reached over SSH with Link's certificates, which its first boot
trusts, and is made with no key in its `ssh-keys` metadata. Another
computer's key is merged in there while the box is made to trust that
computer's certificates, and `remove-keys` takes Link's out again. The
host keys its first boot pinned are kept in its metadata too,
`lnk-host-keys`.

### Its firewall rules

Link makes two firewall rules per project for machines tagged
`lnk-box`, shared by its boxes and deleted with the last one:
`lnk-box-ssh` lets SSH in, and `lnk-box-deny` refuses everything else,
ahead of the default network's own rules, which let in every port from
inside the network, RDP and ping. Both are declared once, in the cloud
contract's `ownership/firewall.toml`, which a project's setup also reads
to make them for journaled launches.

### A box that ends itself

It gets its own job identity, granted only on its VM, and its cloud
deadline in its first boot; the identities are reclaimed once their VM
is gone.

### Journaled Launches

A launch with `LNK_RESOURCE_JOURNAL` instead requires the existing `lnkBoxEnd` role to grant only `compute.instances.delete`.
It preserves that role and other jobs, records the project number and a fresh
nonce before making its service account, and records its immutable account ID
before the exact VM-deletion grant. Lost replies leave durable cleanup evidence.
Default-network launches also require the two existing Link firewall rules
and preserve them during cleanup. The adapter explicitly advertises this
job-identity capability.

### What it attaches

Resources a box uses that it doesn't own are looked up and attached
only, never created or deleted.

## Images

An image is `lnk-<name>` in the account's project, labelled
`lnk-image=<name>` and `lnk-owner=<owner>`, and `lnk-hash=<hash>` when
it's saved with a hash.

### Where an image lives

An image is the whole project's, so a box from it can be in any zone.
Only the project's own images are ever used or deleted.

## Buckets

Bucket labels are Link's labels, and a key reaching only one bucket is
a service account of its own (`lnk-<...>`) given
`roles/storage.objectAdmin` on that bucket and nothing else.

## Prices

What Google charges in a region, from its Cloud Billing Catalog API
(`services/<Compute Engine>/skus`, reached with the account's own
`gcloud` sign-in, as the API has no `gcloud` command): each machine
type's on-demand price an hour, from its family's core and memory
rates, a GB-month of a balanced and an SSD persistent disk, and an
external address an hour.

### How often

The catalogue reader uses the configured per-page byte limit in
`price_catalog.toml`, because a full catalogue page can exceed the
generic HTTP client's cap. It follows page tokens within a fixed page
budget. An incomplete catalogue, a repeated token or an oversized page
is refused and never cached as a complete quote.
Completed catalogues are kept in the cloud contract's shared price cache.
A machine type whose family's rates aren't there, or with GPUs, gives none, never
a guess.
