# Installation Identity

> Installation Identity That Stays Stable

An installation is Link under one OS user; the machine it runs on is
hardware. This installation keeps its stable ID and name in
`~/.lnk/safe/installation.toml`,
readable only by you (600). Read the file in any editor before changing
it, and keep its existing identity when changing a name.

```toml
# ~/.lnk/safe/installation.toml
id = "a1b2c3d4e5f6"
name = "mac"
```

## Stable Identity

`id` never changes when the installation is renamed. Output writes it
as `installation:<id>`. Concurrent identity
creation shares a stable writer lock, so callers agree on one identity.
An existing document requires both fields and refuses unknown fields.

## Settings Owned by Modules

[Machine-wide agent policy](../../../../modules/agents/agent/docs/agents/more/settings/README.md) lives in `~/.lnk/safe/agents/policy.toml`.
[Keeper settings](../../../../modules/security/secret/docs/config/secrets/README.md) live in `~/.lnk/safe/secrets/preferences.toml`.
Each module reads only its own typed settings document. Installation identity
contains no module settings, and an incomplete identity is an error.

## Machine Kind

Root's `/etc/lnk/kind.toml` identifies a box's `os` and `arch`. The OS
labels are `ubuntu-24.04`, `ubuntu-22.04`, `debian-13` and `macos`.
These labels describe the guest. Each provider separately refuses an
OS it cannot supply, and the cloud contract accepts only its
Ubuntu image set.

<!-- spec-reference:start -->

## Checked Reference

These fields are checked against the released type. An unset optional
value waits for its feature to resolve it.

```text
root: id, name
/etc/lnk/kind.toml: arch, kind, os
```

<!-- spec-reference:end -->

## Machine Decisions

[Decisions](installation/decisions.md) explains how the installation identity is chosen
and retained, so callers can distinguish stable identity from display names.
