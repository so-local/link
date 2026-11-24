# Delete a Box

`lnk box delete` ends a box for good, deleting its machine and its disk
after asking. This page says what goes with the box, what stays, and how
to confirm the box is gone.

## What Deletion Does

`lnk box delete` deletes the machine and its disk, after asking, since
everything on the box is lost. On AWS the instance is terminated, its
volume deleted with it, and on Google Cloud it's deleted with
`--delete-disks boot`, its boot disk with it. Local Link then removes what it made for
boxes in that place, such as a firewall rule or a key, once no box of
yours there uses it. Each module that gave the box something, such as a
key to its storage, takes it back, and what one can't is said. The box's
pinned host keys are forgotten, since its address can come back as
another machine's.

```sh
lnk box delete aws-1
```

### Deleting a Box Here

A box here has no machine to delete. `lnk box delete` stops its
environment's services and removes its record, then asks again before
it removes the box's home folder, since that folder is your data.

## Delete by Cloud ID

A machine Local Link no longer has a record of can still be deleted by
its cloud ID, with the account and the region or zone it's in. A machine
already gone counts as deleted, so the command can be run again safely.

```sh
lnk box delete --id i-0abc123 --cloud aws --zone eu-west-2 --yes
```

## What Stays

Deleting a box never deletes what lives apart from it. Your saved images
stay until `lnk box image delete`, resources the specification attached
stay as they were, and snapshots or anything else you made in the
account yourself stay too.

## Confirm the Box Is Gone

`lnk box delete` waits for the cloud to accept the deletion, and says
the box is gone with its disk. `lnk box list --refresh` then asks each
cloud again, and a deleted box is no longer listed. Your cloud's own
console shows the same. An AWS instance stays visible there as
terminated for about an hour, with no disk and no charge, and a Google
Cloud instance disappears once it's deleted.
