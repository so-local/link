# Outside Module Releases

A module from outside Link comes from its maker's own signed release,
never from Link's. This page follows how `lnk` reads that release, while
[Modules from Outside Link](../../../../../../docs/more/modules/security.md#modules-from-outside-link)
says what you trust when you add one.

## Where the Release Comes From

[`lnk mod add --url <url>`](../../../../../../docs/more/modules/guide.md#add-or-remove-a-module) takes a GitHub repository's latest release
(`https://github.com/<owner>/<repo>` only), its `SHA256SUMS` signed
with that repository's own key in the `link-release` namespace, and
each archive checked against it. Link's keys never vouch for it. The
key a release offers is its `release-key.pub`, which the first `add`
shows by its fingerprint before it trusts it.

## What an Outside Module Must Answer

A module from outside Link must describe itself as a module
(`--link-describe`), never a program of its own, before it's installed,
and its name can't be one of Link's modules'. The
[Module Protocol](../protocol.md#lnk-id---link-describe) defines that
description.

## Newer Releases Only

[`lnk upgrade`](../../../../../../docs/more/modules/guide.md#upgrade), and `add` again (as a box does on every move), take
the module's latest release and install it only when it is newer than
the one installed. One whose version, or the installed one's, can't be
compared is refused. A release whose signed `VERSION` file says it is
no newer is neither downloaded nor run.

## Names a Release Lists

A `SHA256SUMS` line naming anything but letters, digits, `.`, `_`
and `-` is dropped, so the names a release not yet trusted lists
reach the terminal only as plain text.
