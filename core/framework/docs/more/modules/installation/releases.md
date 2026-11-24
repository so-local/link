# Signed Releases

How a release is signed, who checks it, and what keeps an older one
from coming back. [Module Installation](README.md) puts these checks in
the order `lnk` runs them.

## Signing

Every release's `SHA256SUMS` is signed by Link's signer, `link`
(`ssh-keygen -Y sign`, namespace `link-release`). Its public line is in
`scripts/release/link-signers`. It is built into `lnk` and embedded in
`install.sh`. Repository checks keep those public-key copies aligned.
No release carries the signer's secret.

## Who Checks

[`lnk upgrade`](../../../../../../docs/more/modules/guide.md#upgrade) and the default `install.sh` path require a valid
signature, so controlling the releases repository or its token alone
does not authorize a new program.

- `lnk upgrade` uses its own SSHSIG verifier, `link_core::release`.
- `lnk upgrade --check`, which the commands that show
  [update notices](../../../../../../docs/more/modules/guide.md#update-notices)
  start in the background at most once a day, reads the latest release's
  `VERSION` only through its signed `SHA256SUMS`, so a notice can't
  name a version Link didn't release. It asks the releases host for
  three small files and sends nothing but `lnk`'s version, the name it
  gives the host. `LNK_UPDATE_NOTICE=off` turns it off.
- `install.sh` uses `ssh-keygen`, and refuses to run without it unless
  `LNK_INSECURE=1`.

## Installer

`curl -fsSL https://local.link/install.sh | sh` fetches the bootstrap
over HTTPS. Its source is `core/cli/install.sh`, and it checks the
downloaded release against its embedded signer. The first script
fetched still relies on its host's HTTPS and trusted build.

## Downgrades

No signed release older than the one installed is put back, unless
you ask for that version.

- `lnk upgrade` refuses a release older than itself unless that version
  is requested, and refuses a version it can't parse.
- `lnk upgrade <version>` installs only that version: the `lnk` it
  downloads must report it, so an older signed release moved under the
  tag is refused.
- Releases carry a `VERSION` file inside the signed `SHA256SUMS`. A
  release without one is refused by `lnk upgrade`, [`lnk box create`](../../../../../../modules/compute/box/docs/services/boxes/guide.md#create-a-box) and
  `install.sh` alike ([`lnk mod add`](../../../../../../docs/more/modules/guide.md#add-or-remove-a-module) asks for its own version).
- `lnk upgrade` checks the `VERSION` file against the version asked for
  and the `lnk` it holds. `lnk mod add` checks it against its own
  version. `lnk box create` checks it as `lnk upgrade` does, the latest
  no older than a released `lnk` making the box, and has the box
  install the version it checked.
- `install.sh` checks the `VERSION` file against `LNK_VERSION`, and the
  `lnk` it installs must report that version. A stamped minimum version
  also refuses an older `latest`.
- `lnk mod add` also runs each module's `--version`, which must
  match, and must answer when a release has no `VERSION` file.

## Download Limits

Downloads give up after 15 s connecting or 60 s without data. They stop
past 64 KiB for `SHA256SUMS`, its signature and `VERSION`, and past
200 MiB for an archive.

## Release Workflows

The release's workflows keep the signer away from third-party
code:

- Workflow actions are pinned to commit hashes.
- Jobs that run third-party actions get no secrets.
- Signing and publishing run in separate jobs that use only `actions/*`.
- Default permissions are read-only.
- A publish fails if the signature doesn't verify against
  `scripts/release/link-signers`.
