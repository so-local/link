# Harness Permissions

**A harness holds what it needs beyond your agent's grants for itself. Local Link asks when your agent switches to the harness, and the harness holds the grants only while it runs.** Your agent's
own grants are on [Permissions](../../../../tooling/sandbox/permissions/README.md).

## Allow What a Harness Needs

Local Link never gives a harness's permissions to another harness. A harness that needs nothing, such as Link
Harness, runs with your agent's grants alone. DeepSeek asks for its install
folder and the toolchains its tools run, each naming its programs.

```sh
lnk agent harness set openclaw
lnk agent harness set openclaw --yes
lnk agent harness-permission list --harness openclaw
lnk agent harness-permission remove --network.mode host --harness openclaw
lnk agent harness-permission add --network.mode host --harness openclaw
lnk agent harness reset --harness openclaw
```

## Read What a Harness Says It Needs

A harness says what it needs beyond Local Link's defaults, and why. The need can
be a sandbox's specification, such as `[network] mode = "host"`, shown as flags. It can also be a risk with its own name, like OpenClaw's `browser`.
`lnk agent harness-permission add` and `remove` take the same flags as `lnk agent
permission add`, or a risk's name. Each refuses a risk the harness never
names, and says so. A mistyped name never reads as a risk already taken back.

## How `lnk agent harness set` Asks

`lnk agent harness set` shows every line the new harness needs that your agent hasn't, and what
the harness was given here before. Then it asks before anything changes. A line about what the harness may
do less of asks nothing. The first start on a
harness, and any start after its needs grew, ask the same way before the harness
runs.

```
! openclaw may do more on this machine than link.

  GRANT                IT OPENS                                        WHY
  --commands.toolchains node  a toolchain's programs, beyond your [commands]  openclaw says: it runs the Node it installed in its home, and the commands its tools run

  This holds for main, and every other agent here that runs openclaw.

No keeps it on link, and nothing changes.
Switch main to openclaw with these? [y/N]:
```

## Choose No or Yes

No, or Enter, changes nothing. Your agent stays on the harness it
had, running, and the next start asks again. Yes grants exactly those lines to the
harness, for every agent here that runs the harness, and starts the harness. What your
agent has of its own asks nothing. A harness needing your home
(`--files.read '~'`) runs without a question for an agent you granted that.

## Answer Without a Terminal

`--yes` answers for you. Without it, the switch or start fails and
says so. Local Link never asks about a harness that needs no sandbox at all. It refuses the
start unless your agent runs with none (`--no-sandbox`),
which `--yes` never implies. Any machine whose trusted
`~/.lnk/safe/agents/policy.toml` says `unsandboxed = false` refuses
`--no-sandbox`.

## Decide on a Risk the Harness Can Run Without

Local Link asks about a risk the harness can run without, such as OpenClaw's `browser`, on its own. No leaves the risk off while the harness runs. Without a
terminal, or with `--yes`, the risk stays off until you add it (`lnk agent harness-permission add`).

## Answer Once

A later start asks only for what's new, never for what you allowed or
took back. `lnk agent harness-permission add` and `remove`, and `lnk agent harness reset`, change
or forget an answer. `lnk agent move` asks in your terminal before the
agent starts where it goes, and no leaves the agent where it was, running.

## Read the Notice at Every Start

Every start of a harness running with more than the defaults says so,
and how to take the grant back, as in `openclaw runs with --network.mode host,
beyond Link's defaults (lnk agent harness-permission remove --network.mode host
--harness openclaw)`. `lnk agent show` shows the same notice in its
`Sandbox` row.
