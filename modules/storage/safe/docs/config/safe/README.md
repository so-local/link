# Safe

A Safe holds the control files that steer Local Link, such as a
sandbox's grants, a module's settings, and Link Models' settings, and
the `safe` module is how they're listed, read, and written. A run never
reaches a Safe by any path, so nothing a run does can change what
steers it.

## What a Safe Is

A **control file** steers Local Link: a setting, a specification, a preset, a
grant, a credential, or an approval. Every control file lives in a Safe,
`~/Link/Safe` on a machine, with one folder for each owner, named by
the owner's kind and name, as `~/Link/Safe/Models` for Link Models.
Local Link's own settings live there too.

### Safe IDs

Each owner's Safe is a resource, `safe:<id>`, named by an ID that never
changes, minted when the owner's Safe is made and kept in the Safe's own
record, so a grant on one owner's Safe covers that Safe alone.

## List, Read, and Write

The `safe` module's actions each name a Safe and a path within it:

- **`safe.file.list`:** lists a Safe's files, each with its revision.
- **`safe.file.read`:** reads one file's text and its revision.
- **`safe.file.write`:** writes one file, refused with `conflict` when
  the revision it was read at has moved on.

[`read` is a `find`](../../../../../../packages/actions/docs/more/developing/modules/catalog/actions.md#effects), and `write` is an `update`.
Text files alone are read and written. The actions are offered over
REST, each a request naming the Safe's ID and the file's path, as
`GET /_link/safe/file/read?safe=safe:q2r3s4t5v6w7&path=sandbox.toml`.

## Checked by the Owning Module

A Safe's files belong to the module that owns them. That module
declares the check a write runs, and `safe` runs it by asking the
module's command, in `safe`'s own types, so `safe` names no module above
it. A write the check refuses keeps the file's last good version and
answers with the reason, so a file written here is checked as the
owning module checks it when it makes the thing the file steers.

## Never a Run's

`safe` declares its actions as control, with the action-level `control`
key the [actions](../../../../../../packages/actions/docs/more/developing/modules/catalog/actions.md) catalog checks.
Local Link refuses any grant that would give a service one of them, so
no run, box, or job is given a Safe, whatever role it holds. The owner
reaches their Safes through their own assignments, and no other path
writes a control file.
