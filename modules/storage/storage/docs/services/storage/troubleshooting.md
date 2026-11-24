# Troubleshooting

What to do when [a `lnk storage` command](README.md) refuses or misses files. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk storage list
```

## "is in none of Link's folders"

Move the files into `~/Link/Files` or an owner's folder: `lnk
storage` takes only paths in them.

## "Leaving out the folders the ... module keeps"

A module that keeps owners, such as `agent`, failed when asked which
folders are its owners'. The warning ends with what it last said. The
command goes on with your files, and refuses a path in that module's
folders. Fix or update the module, then run the command again.

```sh
lnk upgrade
```

## `clean` or `pull` refuses in an owner's folder

`pull` and `clean` refuse while the folder's owner runs, or when the
module that keeps it can't tell, since it could change the files
meanwhile. Stop the owner with that module's command first.

## "names no owner here"

`--owner <kind>:<name>` is resolved by [the module that keeps that kind](../../../contract/docs/services/storage/more/contract.md#owners).
With no such module installed, or no owner of that name, there's nothing
to name: install the module, or check the name with its own list.

## `tree` misses files from another machine

`tree` works offline, from the manifest this machine keeps. `lnk storage list --tree
--refresh` lists the clouds again first, and finds them.

## "that password doesn't open the files already in ..."

`lnk storage connect` and `adopt` refuse a password that doesn't decrypt a file
already there: use the bucket's original password.
