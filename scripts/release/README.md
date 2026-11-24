# Release Inputs

A release reads its inputs from the workspace. The inputs are the keys that
sign a release and a staging build, the advisories a release seals, and
`set-version.sh`, which sets every package's version.

## Code

- [`advisories.toml`](advisories.toml): the advisories a release seals
- [`link-signers`](link-signers): the keys that sign a release
- [`set-version.sh`](set-version.sh): sets every package's version
- [`staging-signers`](staging-signers): the keys that sign a staging build
