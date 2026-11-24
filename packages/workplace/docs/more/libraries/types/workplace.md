# Workplace

`link-workplace` is where work goes, as one type, parsed once from the slots every command shares. A command flattens `Slots` into its arguments, asks for the `Target`, and never reads the place positional, `--on`, `--cloud`, `--zone` or `--here` itself.

## The slots

A command takes at most one slot, and `--here` is the default where a command has one. `Slots::target` answers `None` when none is given, and `Slots::target_or_here` answers `Target::Here` then.

### `--on`

`--on <machine>` is a machine by its real name: a box on any cloud, or a cloud account that is one machine, as its adapter says. It parses to `Target::On`. `--on here` is refused, because here is this machine, and a name that is a cloud account and no machine is refused naming `--cloud`.

### `--cloud`

`--cloud <cloud>` is a cloud account from [`lnk cloud list`](../../../../../../modules/cloud/cloud/docs/services/cloud/guide.md#connect-a-cloud-account), such as `aws` or `gcp`. It parses to `Target::Cloud`, with its zone when `--zone` is given.

### `--zone`

`--zone <zone>` is a region or zone of the cloud, and only goes with a cloud, named by `--cloud` or the positional. A cloud's own word for it is kept, so only its characters are checked. A cloud account that is one machine has no zones, so `--zone` with one is refused when the caller says which accounts are (`Known::single`).

### `--here`

`--here` is this machine, directly, with no relay needed. It parses to `Target::Here`, and it is the default where a command has one.

### The place positional

[`lnk box create aws`](../../../../../../modules/compute/box/docs/services/boxes/guide.md#create-a-box) is short for `lnk box create --cloud aws`, and `lnk box create here` for `lnk box create --here`. The positional follows the command's own, as in [`lnk agent create work aws`](../../../../../../modules/agents/agent/docs/agents/start/create.md#create-on-a-hosting-account), and parses to the same `Target` as its strict form, so nothing past `Slots::target` knows which was typed. It counts as a slot: `aws --here` is two.

#### Declared once

The shared group `workplace` declares the positional beside its flags in `link-actions`' `shared.toml`, which every generated parser reads, so every command taking the group takes it with the same help. `Slots`, the Rust type those commands share, holds exactly the flags of the `workplace` type in `link-component`'s `shared.toml`. Help, docs and hints write the common form without the flag, and `--cloud` stays the strict form.

### A machine by its name

A command that takes `--on` also takes a machine's name as the positional, so a machine `aws-1` given where a cloud would go means `--on aws-1`. A command with no `--on`, such as `lnk box create`, keeps the positional to clouds and `here`.

#### Reading a name against what exists

`Slots::target` reads the positional as a cloud, since telling the two apart needs what exists. `Slots::resolve` takes the command's `Known` machines and clouds, and reads the positional as `Target::On` where it names a machine and no cloud account. A name that is both is refused with both strict forms, as `shared is both a machine and a cloud account: give --on shared or --cloud shared`. `--zone` with a machine's name is refused, since a zone goes with a cloud.

## Refusals

A refusal is a `Refusal`, and its message names the slot the value belongs in. Two slots give `--on and --cloud both say where`, and `--zone` without a cloud says to give a cloud with it. The positional is `<place>` in a refusal. A value of the wrong type is never guessed: `--on here` says to use `--here`, and `--on local` says it is a cloud, `--cloud local`.

## Telling a machine from a cloud

A machine and a cloud account may share a name, so the slot says which one is meant. When a name is only a cloud, `--on aws` is refused with `--cloud aws`, and when it is only a machine, `--cloud mac` is refused with `--on mac`. `Target::check` makes this refusal, given the `Known` machines and clouds, which the command reads from its own sources. A name that is both is right in either slot.

## The strict form

A `Target` displays in its strict form, as the command line spells it: `--on mac`, `--cloud gcp --zone europe-west4-b` or `--here`. A program passes a target on in this form, while help and Nexts write a cloud's common form, the positional, and a refusal names the positional `<place>`.
