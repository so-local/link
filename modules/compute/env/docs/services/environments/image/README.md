# Environment Images

An environment image is a box's disk with the environment's setup stages
already run, saved in your cloud account. A box made from it skips those
stages, so it's ready sooner, and every box made from it starts the same.

## Create an Image

`lnk env image create` builds the image in a connected cloud account. It
asks before it makes anything, since the cloud bills the box that builds
the image while it runs and the image for its storage. `--yes` answers
yes, and with no terminal and no `--yes` it makes nothing and says to add
`--yes`. `--timeout` ends a build that runs too long, as failed.

```sh
lnk env image create --spec env.toml --timeout 1h --yes aws|gcp
```

### How an Image Is Built

Local Link makes a box for the build, the least machine that works on
the file's OS, and runs each stage saved in the image there, in order,
each in its own sandbox or as root where the file says so. It then saves
the box's disk as the image, with the file's hash, and deletes the box,
whatever happened. A box Local Link never got to delete ends itself at
its deadline: the build's `--timeout` and a quarter hour, or four hours
without one.

### What Goes in an Image

Only the stages saved in the image run while it's built, the ones with
no `when = "start"`. A stage run at every start, the environment's
services, and its secrets stay out of the image, so no secret value is
ever saved in one. The build box is never signed in to your Link
account, so the image holds no login of yours.

## Where an Image Lands

The image is saved in the cloud account you built it in, as one of your
box images, which [`lnk box image list`](../../../../../box/docs/services/boxes/image/README.md#list-images) shows. On AWS it's an AMI (Amazon
Machine Image) and its snapshot, in the region the build box ran in. On
Google Cloud it's an image in the project, usable in every zone.

### Image Names and Tags

An image is named `<name>-<first 8 of the hash>`, after the environment's
`name`, and tagged `lnk-hash=<hash>` with the whole hash. Local Link finds
an environment's image by that tag, never by its name, so two environments
never share an image by accident.

```sh
lnk box image list aws|gcp
```

## Image Hashes

The image's hash is made from what builds the image alone: the file's
name and `workdir`, the OS (the file's, or the box's when it names
none) and the box's processor, the variables, and each stage saved in
the image, its command, its user, its sandbox specification and its
`inputs`. `lnk env check` prints it. A changed lockfile makes a new
image, while a comment, a service or a stage run at every start doesn't,
and a box made from the image is given the file as it is now.

### Built Once

`lnk env create` and [`lnk box create --env`](../../../../../box/docs/services/boxes/guide.md#create-a-box) look for a ready image with
the file's hash, and build one only when there's none. Several starts of
one image begun at once on one computer build it once: each waits for
the build under way, then starts from its image. An environment with no
stages saved in an image needs no image at all, and its box starts from
the cloud's own OS image.

## Build on an Existing Box

An image is always a box's, never your computer's. On a box, `lnk env
image create here` runs the stages saved in the image there, root's
included, for the box's OS and processor. It then prints the
[`lnk box image create`](../../../../../box/docs/services/boxes/image/README.md#create-an-image) that saves the image, which you run on the
computer that made the box, since that computer holds the cloud account.
On your computer it's refused before anything runs, as
[Security](../security.md#a-box-or-your-computer) explains.

```sh
lnk env image create here --spec env.toml
```

## Delete an Image

An environment image is one of your box images, so [`lnk box image delete`](../../../../../box/docs/services/boxes/image/README.md#delete-an-image)
deletes it, with its copy of the disk, after asking. Boxes already made
from it keep their disks. The next `lnk env create` with the same hash
builds the image again.

```sh
lnk box image delete app-3f2a9c01 aws|gcp
```

## Images and Docker Images

An environment image is a whole machine's disk, the kind a cloud starts a
new computer from, and never a Docker image. [Environments and
Docker](../lifecycle.md#environments-and-docker) explains how the two
differ, and how an environment can still run Docker.
