# Decisions

Why `link-core` installs, declares and removes modules the way it does.
The [Modules Decisions](../../../../../../docs/more/modules/decisions.md)
explain the choices a person running `lnk` meets.

## Why Are Modules Separate Programs and Not Cargo Features?

Separate programs let a person install one capability without linking
every other module into its executable. Each module has its own
installation and process boundary. Calls between modules use their
documented commands, so callers still depend on those interfaces.

## What a Module Needs

Where what a module needs, or goes with, is declared.

### Where Is What a Module Needs Declared?

The owning `module.toml` declares dependencies and required commands,
including conditions for a feature or platform. The core's registry
derives installation order from those declarations. Repository checks
compare cross-owner calls with their declared requirements, so a new
call updates its owner's declaration rather than a core table.

### Where Is What Goes With a Module Declared?

Beside what it needs, as `recommends` in its `module.toml`, which the
core's `goes_with()` reads: a box goes with `sandbox`, for example.
[`lnk up`](../../../../../../docs/more/modules/install.md#pick-your-modules) takes a use's modules and what they need, never what goes with
them, so a choice installs only what it names. The first command of a
module whose companions aren't here offers them once, each on its own
line with what it is.

## What an Uninstall Asks

How the core learns what each module keeps without knowing any
module's files.

### Why Does Each Module Report What Uninstall Deletes?

So the core knows no module's files: it asks each installed module
(`lnk-<module> uninstall --parts --json`) what it keeps, by part, and
has it stop its services and delete what was picked (`uninstall
--delete <part>`) before its program goes. A module from outside Link
is only told to stop, once you've said yes: its parts are never
offered, so what it keeps in `~/.lnk` or `~/Link` is `rest`.

### How Parts Merge

Parts of one name merge (the
cloud adapters each keep some of `clouds`), and `rest` is what no
installed module claims, which the core deletes itself: a module
removed before keeps its files there. [`lnk mod remove`](../../../../../../docs/more/modules/guide.md#add-or-remove-a-module) takes what
needs a module with it, the reverse of what [`lnk mod add`](../../../../../../docs/more/modules/guide.md#add-or-remove-a-module) brings.

## Why Does a Published Installer Carry a Version Floor?

`latest` in the releases repo has no version to compare with, so
someone with that repo could point it at an older signed release with
a known hole. A published installer can carry its build's minimum
version and refuse an older `latest`. The source script leaves that
floor unset. Signatures authenticate downloaded release files, while
the first script fetched still relies on its host's HTTPS and trusted build.
