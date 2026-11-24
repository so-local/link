# Saved Images

> Save and Reuse Your Box Images

Save a configured box as an image, then reuse its disk in new boxes in the
same cloud account. You need a connected cloud account and an existing box.
Your provider bills you for the image's stored disk.

## Create an Image

`lnk box image create` checks the box for credentials. Then it stops the
box, saves the disk, and starts the box again. Before you share an image,
read [what the image copies and where credential detection stops](images.md).

```sh
lnk box image create aws-1 work
```

### Image Costs

The image keeps a copy of the disk in your account, billed every month
by its size until you delete it. On AWS it's an AMI (Amazon Machine
Image) and its snapshot, in the box's region, and on Google Cloud it's
an image in the project.

## Create a Box From an Image

`lnk box create --image` starts a new box with the image's disk. The box
gets its own installation ID, SSH host keys and `lnk` login, as any new
box does, and keeps the image's processor, `lnk` and SSH keys. It is
made where the image is, in its AWS region or any zone of the Google
Cloud project, with a disk at least the image's.

```sh
lnk box create aws --image work
```

### First Reads on AWS

On AWS, a box from an image loads each part of its disk from the image's
snapshot the first time it's read, so its first reads are slower than a
stopped box's.

## List Images

`lnk box image list` asks the cloud account you pick for your saved images.
An AWS image belongs to one region. A Google Cloud image works in every
zone of the project.

```sh
lnk box image list aws|gcp
```

## Delete an Image

`lnk box image delete` asks first, then removes the saved image and its
disk copy. It does not delete boxes you already made from that image. Keep
an image while you still want to make boxes with the same installed files.

```sh
lnk box image delete work aws|gcp
```

## Choose an Image Source

Every new box starts from an image source, and your saved images are one
kind. [Box Images](images.md) describes the others, so you can choose
before you create a box.

<!-- docs-tree:start -->

## Docs

- [Box Images](images.md): Which image a new box starts from, and what an image you save holds, refuses and gives each box made from it

<!-- docs-tree:end -->
