# Team Memory

**A team's memory searches its whole folder for its members, on Link
Memory.** Team memory stays off until you turn it on, and Local Link hands it only to the members
granted all of the folder (`.`), read or written. [Teams](../README.md) covers the team itself.

## Turn It On and Off

`lnk agent team-memory start` turns a team's memory on. `lnk agent team-memory stop` turns it off, keeping the folder and what
the memory kept:

```sh
lnk agent team-memory start research
lnk agent team-memory stop research
```

The team specification keeps the choice as `[settings.memory] enabled`. One
server answers every member.

## Install the Engine

When Link Memory's engine isn't on the machine, a member's start says so
before installing the engine, and asks first in a terminal. Where no release
matches the running `lnk`, the install can't succeed, so the warning
names what helps: the engine from the build `lnk` came from, installed with [`lnk up --source <checkout> --module memory-tool`](../../../../../../../docs/more/modules/install.md#pick-your-modules), or the team's
memory turned off.
