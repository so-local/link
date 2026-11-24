# File Security

Control reads refuse aliases into writable data and validate ownership,
permissions and size through the handles used to read the document.

## Control Documents

`fs::read_control` and `settings::read` walk directory components without
following user-controlled links. They check each directory's ownership
and write permissions, then open and read the regular file through its
parent handle. Final symbolic links and files with other hard links are
refused. A missing document becomes defaults only when its existing
ancestor prefix passes the same checks.

### Untrusted Folders

A folder that fails the checks is refused as `fs::Untrusted`: the file, the
folder, its owner and which rule it broke (`Distrust::Owner`, owned by
neither root nor you, or `Distrust::Written`, written by others than its
owner). `lnk` says so as a refusal naming that folder. Inside an
unprivileged user namespace, such as a sandbox's own network, `/` is
owned by the kernel's overflow user, and Link reads no settings there.

### Modules Reading No Control

Before running a module's command, `lnk` checks the folders its settings
are in. A module whose `module.toml` says `reads-control = false` under
`[module]` reads no settings, map, grant or credential, so it skips that
check. Only the offline manual, [`lnk man`](../../../../../../../modules/tooling/manual/docs/tooling/manual/README.md#manual), says so, and a test holds it
to that, so the manual opens where the folders can't be trusted.

### Nothing Handed to Them

Skipping the check can't bring such a module control another way. `lnk`
hands it no secrets, no artifact cache and no list of modules read from
its settings. The manual renders in the default look and keeps no
records, rather than reading the settings that would say otherwise.

## Linked Roots

A home reached through a link, as `/home` is `var/home` on Fedora Atomic
or a network home linked in, is read where `HOME` names it. The strict
read resolves only the root a path is under, the user's home as `HOME`
names it or one of the system roots, once. It then walks the real path
from `/`, checking every folder's ownership and write permissions as
before. A link below the root, such as one in a Safe, is still refused.

## System Aliases

`layout.toml` lists the OS alias locations a strict read may traverse.
The alias and its parent must be root-owned, with the parent writable
only by root. Its target is expanded through the same checked walk,
which refuses cycles and further unapproved links. Root ownership alone
does not permit an arbitrary alias in a root user's granted data.

## Owner Boundaries

Callers select control locations through the typed layout catalogue.
Ordinary input documents may use `fs::read_trusted` with their own trust
policy; that does not make input into trusted control. A pinned parent
keeps subsequent operations relative to that directory if its path is
renamed. Sandboxes separately close Safes and writes to installed programs.
