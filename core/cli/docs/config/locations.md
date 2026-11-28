# Locations

`lnk location` shows where Local Link keeps each of its locations on this
machine, and moves one. The Link folder, `~/Link`, is one of them, and
[Where Link Keeps Things](../../../../docs/more/reference/locations.md)
explains every location.

## List the Locations

Alone, `lnk location` lists the map, `~/.lnk/safe/map.toml`: each location
by its ID, such as `link` for the Link folder, and where it is, starting
with the Link folder.

```sh
lnk location
```

## Set a Location

`lnk location set` moves a location and everything inside it, then
updates the map. Here the Link folder moves into `Documents`:

```sh
lnk location set link ~/Documents/Link
```

When you already moved the files yourself, the same command points Local
Link at their new place. When the Link folder isn't where the map says,
the next `lnk` command asks where it went, or whether to make a new one
with `lnk location create`. Local Link never searches the disk for it.

## Check for Staging Files

Preparing a run cleans expired atomic-write staging from your Safes.
`lnk location check` lists old staging in your data without deleting it,
and `--json` returns an array of paths.
[Abandoned Staging](../../../../packages/files/docs/more/libraries/files/files.md#abandoned-staging)
gives the age, cadence, and scan boundaries.

```sh
lnk location check
```
