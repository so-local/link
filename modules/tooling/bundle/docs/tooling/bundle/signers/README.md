# Signers

A signer is what signs a bundle. It has a secret that stays on the machine
that signs, and a line that every machine installing the bundle trusts.
Name a signer of your own, and the module makes it the first time. The
module keeps the signer in the Safe, where no run reaches it.

```sh
lnk bundle create devbox --signer dev
```

## Make a Signer

`lnk bundle signer create` makes a signer of your own before any bundle
names it. Use it for a source whose `scripts/release/staging-signers` must
name the signer's line first. It refuses a signer you already keep and
Local Link's own signers.

```sh
lnk bundle signer create qa
```

## Where Signers Are Kept

Each signer has a folder of its own, `~/.lnk/safe/bundles/signers/<name>/`.
The folder holds `secret`, which only you read, and `signers`, the signer's
one allowed-signers line, written last. A signer is kept once its line is
written. A run interrupted before then starts again from the start and
replaces the secret. A machine trusts the signer by that line, such as in a
server's own `release-signers` file. `lnk bundle signer show` prints a
signer's folder, its file and its line, and `--json` gives them to another
program. A signer never made is an error that names the command that makes
it.

```sh
lnk bundle signer show qa --json
```

## Use Local Link's Signers

`link` and `staging` are Local Link's own signers. Their lines are in the
public source's `scripts/release/`, `link-signers` and
`staging-signers`, and their secrets aren't made here: a maintainer who
holds one keeps it in its folder, and CI writes it from the signing
environment's secret for one run. Named without its secret, one of them
leaves the bundle unsigned, a part compiled to trust it, which [the job
holding the secret seals](../README.md#seal-parts-built-elsewhere).

### Moving Link's Signer to `link`

Local Link's approved signing secret belongs in the named `link` folder. A
maintainer whose approved secret is kept under `release` can move that
folder to `link`. `--signer release` names a custom signer. It does not
select Local Link's committed signer declaration. Run `mv ~/.lnk/safe/bundles/signers/release ~/.lnk/safe/bundles/signers/link`.
