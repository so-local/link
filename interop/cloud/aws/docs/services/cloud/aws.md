# Amazon Web Services

`lnk-aws` is the AWS adapter: accounts, buckets, boxes and images on
Amazon Web Services, through the `aws` CLI. `lnk cloud`, `lnk box` and
`lnk storage` run it by [the cloud contract](../../../../../../modules/cloud/cloud/contract/docs/services/cloud/more/adapters.md);
nobody types it. Connect the AWS account you want Link to use.

```sh
lnk cloud connect aws
```

## Adapter Interface

The AWS adapter implements the requests defined by
[Cloud Adapters](../../../../../../modules/cloud/cloud/contract/docs/services/cloud/more/adapters.md).
It reads each request as a typed command line and dispatches it to
AWS's resource APIs. Use the public cloud, box, and storage
commands for interactive work.

## An account

`lnk-aws connect` connects an AWS account from your aws login (a
profile) or an access key, and checks it with AWS. Its questions go to
stderr, since stdout is the account, as JSON.

### Its flags

[`lnk cloud connect aws`](../../../../../../modules/cloud/cloud/docs/services/cloud/guide.md#connect-a-cloud-account) passes its flags on: one type, read from flags
or a specification file alike.

### Its CLI

When boxes need the AWS CLI and it's missing, it installs it for you
with AWS's own installer, from `awscli.amazonaws.com`, into Link's
settings folder (`~/.lnk/safe/aws`), for you only: no administrator rights,
and nothing else on the machine changes.

## Boxes

Each box is an EC2 instance of Ubuntu LTS in the account's default VPC,
named `lnk-<box id>` and tagged with its name, ID and owner, reached
over SSH with Link's certificates, which its first boot trusts.
It's launched with no key pair.

### Another computer's key

Another computer's key is pushed with EC2 Instance Connect for a
minute, for [`lnk box`](../../../../../../modules/compute/box/docs/services/boxes/README.md#boxes) to append over SSH while the box is made to trust
that computer's certificates. `remove-keys` has nothing to do on AWS.
The host key its first boot pinned is kept in a tag, `lnk-host-key`.

### What it shares per region

Link makes a security group per region, shared by its boxes there and
deleted with the last one. An older `lnk box` gives a key too, which
gets a key pair named by its hash (`lnk-box-<hash>`). With the last box
in a region, every `lnk-box-*` key pair there goes too.

### Its security groups

`lnk-box` lets SSH in from anywhere, for a box reached at its port 22;
`lnk-box-tunnel` lets nothing in, for a box reached only through
Systems Manager, whose agent calls out over HTTPS. Each is made the
first time a box needs it and deleted with the last box.
Boxes made at once may each find no group and make it. AWS refuses all
but the first as duplicates, and each of those boxes then uses the group
the first one made.

### What a box is launched as

A box is launched from its image and disk, with its tags and spot
options, as one of the instance types its region offers. Resources a box
attaches that it doesn't own are attached and kept when the VM is
terminated.

## Images

An image is an AMI named `lnk-<name>`, its snapshot with it, both
tagged `lnk-image=<name>` and `lnk-owner=<owner>`, and
`lnk-hash=<hash>` when it's saved with a hash.

### Where an image lives

AMIs live in one region: a box from one is made there. Only the
account's own AMIs (`--owners self`) are ever used or deleted, never a
public one.

## Buckets

S3 bucket tags are Link's labels, and a key reaching only one bucket
is an IAM user of its own with a policy for that bucket and nothing
else.

### Its user's boundary

The user is named as the bucket, under the path `/lnk/`, and made with
the permissions boundary `lnk-bucket-boundary`. That managed policy
allows a bucket credential's actions on the bucket named `${aws:username}`
alone, so any policy the user is given reaches no more than its bucket.
`lnk-aws` makes it in the account the first time. It refuses to make a
key under one of that name whose document isn't Link's.

### Users from before

An older `lnk-aws` named the user `lnk-<bucket>`, with no boundary.
Making the bucket's key again, or revoking it, deletes that user, its
keys and its policy.

## Prices

What AWS charges in a region, from its Price List Query API (`aws
pricing get-products`, the same CLI): each instance type's on-demand
Linux price an hour, a GB-month of gp3 (balanced) and io2 (fast)
storage, a public IPv4 address an hour and a GB out to the internet.

### How often

Prices are read once in a while and kept in the cloud contract's shared
price cache. An answer without a price gives none, never a guess.
