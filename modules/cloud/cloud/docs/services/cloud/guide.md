# Platforms Guide

Connect named cloud accounts, inspect their identities, and disconnect them
without deleting their resources. Choose a saved profile or a service account
when you need separate accounts for work and automation.

```sh
lnk cloud connect aws
lnk cloud list
```

## Connect a Cloud Account

Connecting makes nothing in the account. The same account and its one
sign-in keep your [buckets](../../../../../storage/storage/docs/services/storage/README.md) too.

```sh
lnk cloud connect aws
lnk cloud connect aws --profile work --region eu-west-2
lnk cloud connect aws --name aws-work --key-id AKIAEXAMPLE
lnk cloud connect gcp --project my-project --zone europe-west1-b
lnk cloud connect gcp --name gcp-ci --credential-file key.json
lnk cloud list
```

### Google Cloud

A Google Cloud account uses gcloud's login and project, and signs you in
to gcloud first if you aren't. It signs you in once more for buckets,
because Google's libraries can't use gcloud's own login. A service
account connects by its key instead, which you get from the Google Cloud
console under IAM, Service accounts, Keys.

### This Machine's Own Account

On a cloud's own machine, `--machine` connects the account the machine
itself runs as, and Link keeps no credential for it. `connect` hands the
flag to the cloud's adapter, and Google Cloud's is the one that takes
it: it connects the service account attached to a Compute Engine
machine, and refuses a machine with none, as [its
page](../../../../../../interop/cloud/gcp/docs/services/cloud/gcp.md#this-machines-service-account)
says. `lnk cloud list --json` marks such an account `machine`.

```sh
lnk cloud connect gcp --machine
```

### Without the Cloud's CLI

`connect` offers to install it in Link's settings folder, with no
administrator rights. If you say no, an AWS account, or a Google Cloud
service account connected by its key, still works for buckets but not
for boxes. A Google Cloud login can't connect without gcloud.

### Accounts from a Sign-in

An installed module can expose a cloud account from a sign-in it already
holds. `lnk cloud list` and every command using that account discover it
automatically, with no `connect` or saved duplicate. Sign out through the
module that supplied it to remove that connection.

### Other Clouds

`connect` takes any kind whose adapter, `lnk-<kind>`, Link finds, with the adapter's own settings given as `--setting`. An adapter may say an account it connects is one machine rather than a cloud that makes machines: such an account is named with `--on`, bills no one and has no zones. A name a cloud goes by, its kind or its kind without `-cloud`, belongs to that cloud's own account, and an account of another kind never takes it.

## Show an Account

`lnk cloud show` shows one account's kind, place and the identity it signs in as, and never its credential.

```sh
lnk cloud show aws
```

## Disconnect an Account

Disconnecting forgets the account and its key, and deletes nothing in
it.

```sh
lnk cloud disconnect aws-work
```

### What Waits for It

Its boxes and buckets wait until you connect it again, and `lnk storage
disconnect` forgets its buckets. An access key keeps working until you
delete it in the cloud's console.

## How It Runs

Each cloud's adapter runs as its own program, `lnk-aws` and `lnk-gcp`,
and the commands exchange [typed requests and replies](more/protocol.md) with it.
