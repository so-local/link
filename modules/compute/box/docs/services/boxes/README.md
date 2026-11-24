# Boxes

> Boxes Are Computers in Your Own Cloud

Create and manage computers in your own cloud account, with Local Link
installed. A [Box Specification](../../config/specs/box.md) says what machine you
want, as a file or as flags.

<!-- quickstart -->

## Create a Box

Connect your cloud account, then create an Ubuntu machine there. Your
provider bills you for running machines and stored disks. You can also
choose [another cloud, a machine size, or an image](guide.md).

```sh
lnk cloud connect aws|gcp
lnk box create aws|gcp
```

## Open a Shell on a Box

Open a shell on the box you created. Pass the box's name when you have
more than one. You can also [copy files and run commands](ssh.md) over
SSH, and that page explains how Local Link connects.

```sh
lnk box ssh
```

## List Your Boxes

List your boxes, including those you created from your other computers.
Local Link [finds and tracks](guide.md) each box for you.

```sh
lnk box list
```

## Stop and Start a Box

Stop a box when you don't need its compute, and start it again later. A
stopped box still bills for its disk, so [delete the box](lifecycle/delete.md#what-deletion-does)
when you're done with it. Replace `aws-1` with your box's name.

```sh
lnk box stop aws-1
lnk box start aws-1
```

## More

Boxes do more than run a shell. You can size a box before you pay for
it, save a box as an image, and give other people their own logins.

### Follow a Box's Life

[Box Life Cycle](lifecycle/README.md) follows a box from its creation to its
deletion: what runs and is installed on it, its files and records, how
an environment sets it up, and how to confirm a deleted box is gone.

### Choose and Reuse Machines

[Preview the machines](size/README.md) that match your specification before you create a box. Save a configured box as an
[image](image/README.md), and reuse its disk in new boxes.
[Box Images](image/images.md) explains what an image holds and how its
credentials are checked. Give people their own sandboxed logins on a box
with [Box Users](user/README.md).

### Limits and Fixes

Only you can reach your cloud accounts and the boxes in them, under the
assumptions in [Security](security.md). When a box won't create, start,
or save, [Troubleshooting](troubleshooting.md) has the fix, and
[Decisions](decisions.md) gives the reasons behind each choice.

## Implement an Adapter

A new cloud gets boxes through an adapter. The adapter implements the
box [interface](more/protocol.md) and the
[Box Contract](../../../contract/docs/services/boxes/more/contract.md),
which [More](more/README.md) describes.

<!-- docs-tree:start -->

## Docs

### Pages

- [Boxes Guide](guide.md): A computer in your own cloud, running `lnk`
- [Saved Images](image/README.md): Save a configured box as an image, then reuse its disk in new boxes in the same cloud account
- [Machine Sizes](size/README.md): Preview the machines that match a box specification before you create a box
- [Box Users](user/README.md): Give people Linux logins on a box, each confined by a sandbox specification
- [Box Life Cycle](lifecycle/README.md): A box lives from `lnk box create` until `lnk box delete`, and this page follows it through each stage
- [Reach a Box](ssh.md): `lnk box ssh` opens a shell on a box, or runs one command there, over OpenSSH with Local Link's own key and a certificate that lasts an hour

### More

- [More](more/README.md): An adapter implements the operations behind the box commands
- [Decisions](decisions.md): Why clouds and boxes work the way they do
- [Security](security.md): What's protected: your cloud accounts, whose keys `lnk cloud` keeps, and the machines `lnk box` makes in them, against anyone but you reaching them
- [Troubleshooting](troubleshooting.md): What to do when a box won't make, start, show or save, symptom by symptom

<!-- docs-tree:end -->
