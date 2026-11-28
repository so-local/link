# Platforms

> Platforms for Your Boxes and Buckets

**Connect your [AWS](../../../../../../interop/cloud/aws/docs/services/cloud/aws.md)
or [Google Cloud](../../../../../../interop/cloud/gcp/docs/services/cloud/gcp.md)
account once, and your boxes and buckets there use it.** Local Link
connects the account through an adapter, lists your accounts, and
removes a connection when you ask.

<!-- quickstart -->

## Connect a Cloud Account

Connect your Amazon Web Services or Google Cloud account. Connecting makes
nothing in the account. The guide covers the [complete
workflow](guide.md#connect-a-cloud-account).

```sh
lnk cloud connect aws|gcp
```

## List Your Cloud Accounts

List the connected cloud accounts and the identities they sign in as. The
guide explains [multiple accounts and credential
selection](guide.md#connect-a-cloud-account).

```sh
lnk cloud list
```

## Disconnect a Cloud Account

Disconnect an account by the name that the list shows. Disconnecting
deletes nothing in the account, and its boxes and buckets wait until you
connect it again. The guide explains [what
waits](guide.md#disconnect-an-account).

```sh
lnk cloud disconnect aws
```

## Build on Cloud Accounts

Other programs can rely on the commands that [`lnk cloud` offers](more/protocol.md),
whichever adapter answers them.

## Contribute Here

An adapter for another cloud lets Local Link run boxes and keep buckets
there, beside Amazon Web Services and Google Cloud.
[Cloud Adapters](../../../contract/docs/services/cloud/more/adapters.md)
defines every command a cloud adapter answers, along with the ownership
rules for its resources.

<!-- docs-tree:start -->

## Docs

### Pages

- [Platforms Guide](guide.md): Connect named cloud accounts, inspect their identities, and disconnect them without deleting their resources

### More

- [Interface](more/protocol.md): What other programs can rely on from `lnk cloud`, and where the commands a cloud adapter, `lnk-<kind>`, answers are

<!-- docs-tree:end -->
