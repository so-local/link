# Report a Vulnerability

Report it privately, with **Report a vulnerability** on the repository's
[Security tab](https://github.com/so-local/link/security/advisories/new)
or by mail to **so@local.link**. Please don't open a public issue or pull
request for it.

## What to say

Say what is affected (`lnk`, a module, `install.sh`, or the
documentation website) and how to reproduce it.
You'll get an answer within a few days, and fixes are credited in the
release notes unless you'd rather not.

## What gets fixed

The latest release of `lnk` and its modules receives fixes, which [`lnk upgrade`](modules/guide.md#upgrade) installs. A report of a gap already listed on these pages is still welcome
if you see a way to make it worse than described.

## Security update notices

Releases include advisories authenticated by their signed checksums.
`lnk upgrade --check` reports advisories affecting the installed version
and keeps them for subsequent commands. A critical advisory appears on
stderr every command, including with JSON output, without a terminal and
with `LNK_UPDATE_NOTICE=off`. It names the fix and `lnk upgrade`. Notices
never block commands or wait for the network, and failed checks retain
known guidance. Recommended advisories follow the ordinary notice policy.
