# Settings

> Shared Application Settings

`link-settings` owns Home's locator, folder moves and application settings, so
commands share their saved output settings without depending on modules.

## Folders

`folders::Settings` reads the user's chosen Link folder and output
settings. `link_dir` and `try_link_dir` locate the Link folder; use
the fallible form when an invalid home or settings file must refuse an
operation. `folders::Folder` re-exports the shared layout root type; use
`link_files::layout::{Home, Hidden}` for managed locations within those roots.

### First Need

Inspection creates no folder, marker, or map. `folders::inspect` reads
the current location, and `folders::need` creates and protects a needed
Home before remembering it. Before untrusted code starts,
`execution::prepare` prepares those roots and the runtime socket root
before sandbox masks resolve, then sweeps abandoned Safe
staging.

#### Records and Addresses

Saving model runtime addresses and writing the first unexpected
failure record also need Home, so they create its marker and remember its
identity. Selecting output settings or inspecting records creates nothing.
A missing or readable locator takes the normal first-need path,
remembering an existing marker before logging, so a later move cannot
leave its identity unknown.

#### Failure Record Boundaries

A recorded Home whose marker is missing or moved stays lost, and failure
logging cannot recreate it. If the locator cannot be parsed, logging can
still use an existing marked Home selected by the trusted map. It does
not repair the locator, settings, marker, or map. An invalid map, an
untrusted locator, or a readable locator naming another identity refuses
that destination.

### Relocation

Folder moves and arrived homes rewrite text through held directory and
file handles, preserving file modes and replacing each changed file
atomically. Reads are bounded by the package's `relocation.toml`;
binary files, SQLite files and their companions are skipped. The
[relocation boundary](settings/security.md) explains link refusal and
concurrent changes.

## Command output

`output::run` and `output::exit` configure `link-format` with the saved
format, verbosity and record settings before running a command.
`output::machine` and `output::records` expose those selections for
commands managing output themselves. Import formatting types from
`link-format` directly.

## Damaged Files

Without the machine's locator, the default Link folder's existing marker
supplies its identity again, while its saved settings and data stay in
place. A recorded folder whose marker is missing stays lost instead of
receiving a new identity. Malformed settings files fail with their path
and remain untouched. Restoring the original marker or valid settings
allows the next read to succeed without changing unrelated data.

## Settings Decisions

[Decisions](settings/decisions.md) explains why a person's settings stay apart
from installation identity and how that boundary constrains callers that write them.
