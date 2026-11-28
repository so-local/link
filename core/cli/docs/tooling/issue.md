# Issues

`lnk issue create` turns the last command that failed into a bug report,
scrubbed of what's private, so you can report it in a minute. It's part
of `lnk` itself, so it works with no module installed.

## Report a Bug

Run `lnk issue create` right after the command that failed. It prints
the fields of Local Link's
[bug report](https://github.com/so-local/link/issues/new?template=bug_report.yml)
for you to read and paste. `--open` fills them in on the form in your
browser instead. Link sends nothing itself: you read the report and
choose to submit it.

```sh
lnk issue create
lnk issue create --open
```

## What a Report Holds

A report holds which `lnk` ran, the platform, the command as you typed
it, when it failed, and what went wrong. Names you chose become their
kind and a number, such as `<agent 1>`. Paths, addresses, your username,
and anything shaped like a secret are replaced or left out, as
[Into an Issue](../../../../packages/format/docs/more/libraries/runtime/format.md#into-an-issue)
describes.

## When There's Nothing to Report

Only a failure that's Link's own fault writes a record: a crash, or an
error no command explained. A usage error or a refusal ends on what to do
next, with nothing to report. A security problem is never an issue:
[Report a Vulnerability](../../../../docs/more/vulnerabilities.md) says
how to tell us privately.
