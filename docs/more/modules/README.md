# Link Modules

> Modules for Only the Parts You Use

Local Link is modular, so you install only the parts you use. Those
parts are [Link modules](../architecture/glossary.md#link-modules), managed through
the `lnk` command. Get `lnk` onto your machine and choose the first parts you
need with [Install](install.md).

<!-- quickstart -->

## Install and Inspect Modules

[`lnk mod`](guide.md#add-or-remove-a-module) lists, adds and removes
Link modules, and adds the other parts each needs. The
[complete workflow](guide.md#add-or-remove-a-module) has the current command
spellings.

```sh
lnk mod list
lnk mod show telegram
lnk mod add telegram
```

## Upgrade Modules

[`lnk upgrade`](guide.md#upgrade) updates `lnk` and every installed
Link module, and only from a release whose signature `lnk` checked. The
[complete workflow](guide.md#upgrade) is in the guide.

```sh
lnk upgrade
```

## Look Up Module Interfaces

The core and an executable talk through the
[Module Protocol](../../../core/framework/docs/more/modules/protocol.md),
which covers discovery, questions, installation, and removal.
[Module Contracts](contracts.md) points to each subject's wire shapes. A caller
tells refusal from failure by the
[exit codes](../../../packages/format/docs/more/modules/exit-codes.md).

## How Installation Is Built

`link-core`, the package behind `lnk mod` and `lnk upgrade`, finds each
module's release, checks its signature, and refuses an older one.
[Module Installation](../../../core/framework/docs/more/modules/installation/README.md)
explains that machinery for a reader who wants to see how it works.

## Trust and Recovery

Release verification and the trust placed in outside modules are in
[Security](security.md). Diagnose installation and update failures with
[Troubleshooting](troubleshooting.md). Read why the programs are installed and
updated this way in [Decisions](decisions.md).

<!-- docs-tree:start -->

## Docs

### Pages

- [Modules Guide](guide.md): `lnk` is a small core, and each part of Link is a module, so you install only the parts you use
- [Module Contracts](contracts.md): A module exposes a versioned interface
- [Install](install.md): Install `lnk`, then pick the parts of Local Link you want

### More

- [Decisions](decisions.md): Why `lnk` is installed, split and updated the way it is
- [Security](security.md): Release installation verifies the signed programs Link publishes
- [Troubleshooting](troubleshooting.md): What to do when [`lnk` doesn't install](install.md), or a module won't install or update

<!-- docs-tree:end -->
