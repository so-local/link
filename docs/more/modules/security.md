# Security

Release installation verifies the signed programs Link publishes.
A module from outside Link is checked against the maker's key you trusted.

## Which Program Runs

`lnk <group>` runs `lnk-<module>` from `lnk`'s own folder first, then
from the PATH, the way `git` does. So whoever can write to a folder on
the user's PATH can already replace what the command runs. Empty
and relative PATH entries, such as the current folder, are skipped:
`lnk` drops them from the PATH it runs with and hands its modules, so
neither runs a `tar` or `systemctl` from the folder it was started in.

## What Installs Modules

Release installs check signatures and checksums. A
[source installation](../developing/setup/install.md#cargo-from-source)
builds the selected checkout instead, so its trust comes from that source.

- [`lnk up`](install.md#pick-your-modules), [`lnk mod add`](guide.md#add-or-remove-a-module), a missing module's install offer and `lnk
  upgrade` install Link's modules only from a release whose `SHA256SUMS` is
  signed by the keys built into `lnk`, and check each archive against
  it.
- `lnk mod add` takes the release of `lnk`'s own version, so modules
  and core match.
- In a terminal, a missing-module offer asks first unless the command
  already names the module. Outside a terminal, a direct command can
  install the modules its declared operation needs. A module's nested
  call installs nothing and names the missing prerequisite.
- When an enabled agent tool is missing, a release build can install
  `lnk-run-tool`, `lnk-file-tool`, or `lnk-code-tool` from the signed
  release of `lnk`'s own version. A source build requires the tool's
  executable to be available from the same checkout.
- `install.sh` installs `lnk` alone.
- Link's modules come only from Link's own releases. Prebuilt programs
  exist for macOS (Apple Silicon, Intel) and Linux (x86_64, arm64)
  only; other platforms build from source.

## Modules from Outside Link

A module Link doesn't build is checked against its maker's key, which
you trust once and `lnk` pins. [Outside Module Releases](../../../core/framework/docs/more/modules/installation/outside.md)
follows how `lnk` reads its release, what the module must answer, and
what it refuses.

- The first `add` trusts the key the release itself offers
  (`release-key.pub`), after showing its fingerprint and asking, or
  `--yes`. Whoever controls the repository on that first day is
  trusted: compare the fingerprint with one its maker publishes
  elsewhere.
- `lnk` pins the repository and the key in `~/Link/Safe/settings.toml`
  (600). Every later `add` and [`lnk upgrade`](guide.md#upgrade) of it verifies against the
  pinned key alone: a release signed by another key is refused, and a
  key is only replaced by `remove`, then `add` again.
- An outside module runs as you, outside any sandbox, like any program
  you install: the harness module asks it what to run, and `lnk` asks
  its version. Link's sandbox confines the harness it installs and
  runs, not the adapter's own answers.
- On a box, [`lnk agent move`](../../../modules/agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud) adds it with `--key`, the key this
  computer pinned, so the box trusts nothing the release offers.

## Uninstall

An uninstall deletes only inside Link's two folders, and shows what
goes before it asks.

- [`lnk uninstall`](guide.md#uninstall) deletes only inside `~/.lnk` and `~/Link`. A
  path a module names outside them, with `..` or relative, is left out.
  A path whose folder leads out of them through a link is left out too.
- Before asking, it lists every path each picked part deletes.
- Before asking, it runs only Link's own modules. Another program named
  `lnk-*` in `lnk`'s folder is listed, then told to uninstall and
  removed once you say yes. It's never asked to delete a part.
- [`lnk mod remove`](guide.md#add-or-remove-a-module) keeps a module that fails to stop what it runs,
  and shows what the module said, so its services never outlive the
  program that stops them.

## Releases and Updates

Every release Link publishes is signed, and `lnk upgrade` and
`install.sh` put nothing in place whose signature doesn't verify. No
signed release older than the one installed comes back unless you ask
for that version. [Signed Releases](../../../core/framework/docs/more/modules/installation/releases.md)
covers the signer, each program that checks it, downgrades, download
limits, and the release workflows.

## Gaps

What these checks don't cover.

- The first `lnk mod add --url <url>` trusts whatever key the repository
  serves that day; nothing in `lnk` vouches for an outside module's key.
- An outside module can go back to an older release signed by the same
  key only if its maker publishes it as the latest; `lnk upgrade`
  refuses a version older than the installed one.
- An outside module's release without a `VERSION` file, or whose
  `VERSION` says newer than its program does, has its program run
  (`contract`, then `--version`) before it's refused as older. It is
  signed by the pinned key, so this runs nothing its maker couldn't
  run already.
- Link's signer is a GitHub Actions secret; see
  [Known limitations](../architecture/security.md#known-limitations).
- A source checkout's `install.sh` has no version floor, so `latest`
  pointed at an older signed release with a `VERSION` file is installed.
  Website builds and sealed releases stamp their own minimum version;
  the first script fetched still relies on its host's HTTPS and trusted build.
