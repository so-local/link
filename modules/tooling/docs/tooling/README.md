# Tooling

> Tools That Make Local Link Easier to Use

**Tooling is what developers use around Local Link to make it easier
to use.** Its tools keep a program to the folders and hosts you allow,
measure what each agent costs, read this manual offline, build Local
Link's programs into one signed folder, carry agents' state in one
file, and turn a failure into a bug report. Each tool is a module of its
own, but the bug report, which is part of `lnk` itself.

<!-- quickstart -->

## Run a Program in a Sandbox

Give a program only this folder, then see what else it can reach. The
[sandbox](../../../security/sandbox/docs/tooling/sandbox/README.md)
asks you first when a program wants more, and says where it can't run.
Every agent runs in a sandbox, and so can any other program.

```sh
lnk sandbox check --ready
lnk sandbox run --commands.toolchains shell --files.write . -- bash
```

## See What Each Agent Costs

See what each of your agents costs you as it happens, including memory,
CPU, tokens and spend. [Measure](../../measure/docs/tooling/measure/README.md)
shows current resource use and the costs it kept. It records numbers
only, never a message, prompt, file, tool argument or answer.

```sh
lnk measure
```

## Read These Docs Offline

[`lnk man`](../../manual/docs/tooling/manual/README.md#manual) browses
this documentation in the terminal, the same topics as the website.
[Manual](../../manual/docs/tooling/manual/README.md) reads the installed
documentation offline.

```sh
lnk man tooling/sandbox
```

## Build Local Link's Programs

[Bundle](../../bundle/docs/tooling/bundle/README.md) builds Local Link's
programs from source into one signed folder.

```sh
lnk bundle list
```

## Move Agent State in an Archive

An [archive](../../archive/docs/tooling/archive/README.md) carries your
agents' state between machines and people in one file. It only reads and
writes files, so you can share an agent or start one that someone else
made.

## Report a Bug

[`lnk issue create`](../../../../core/cli/docs/tooling/issue.md#report-a-bug) turns
the last command that failed into a bug report, scrubbed of what's
private, for you to read and paste.

```sh
lnk issue create
```
