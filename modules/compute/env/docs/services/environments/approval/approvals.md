# Approvals

An environment you run is given a secret its file asks for only after your yes, kept for that file.

## Your Yes

An environment you run acts as you, so it can be given the secrets you
hold. It isn't given one until you say so: an approval is your yes to
giving one environment file one secret it asks for.

### Asked by name, kept by ID

An environment file asks for a secret by its name under `[secrets]`, as
`NPM_TOKEN`, which travels with the project. Your yes keeps the secret's
ID, `secret:<id>`, so a secret you rename keeps its approval, and one you
delete and create again with the same name asks again.

### Tied to the file and its name

An approval is for the file's real path, its `name` and the secret. A
changed `name`, or a copy or a move of the folder asks again. Edits and
a replacement file at the same real path with the same `name` retain
permission: an approval records no file digest or inode. Review changes
to an approved file before running it.

### Field flags ask again

Field flags over the file (`--os macos` beside `--spec env.toml`)
make another specification than the one you said yes to: each secret it names is
asked for again, and the yes is that run's alone, never kept. With no
one there to answer, the run is refused; write the change into the file
instead.

## Say Yes

A run at a terminal asks once for each unapproved secret. To record permission
before an unattended run, [Add an Approval](README.md#add-an-approval)
for the exact environment file and secret.

### See what you approved

[List Approvals](README.md#list-approvals) shows the saved records, and [Show an Approval](README.md#show-an-approval) checks one file and secret. The machine-readable view names secrets by stable ID, never value.

### Take one back

[Remove an Approval](README.md#remove-an-approval) revokes saved permission for the next run. Deleting the secret also removes its approvals, so recreating a secret with the same name never inherits permission.

## With No One There

A run no one is there to answer, such as a script's, is refused a
secret with no approval, naming the secret and the `lnk env approval
add` command that gives it. There's no flag that says yes to
everything.

### Jobs

A job runs with no one there, so it's given no secret: one its file or
its environment's names is refused, approved or not.

### Boxes

`lnk env create` and [`lnk box create`](../../../../../box/docs/services/boxes/guide.md#create-a-box) read the approved secrets on your
computer and hand them to the box they make, at its making and at each
start. The box never reads your Safe.

## Kept in Your Safe

The approvals are in `~/.lnk/safe/envs/approvals.toml` (600), in your
Safe, which no sandbox reaches. Each records the file, its name, the
secret's ID and the name the file asks it by:

```toml
# ~/.lnk/safe/envs/approvals.toml
[[approval]]
spec = "/home/me/app/env.toml"
name = "app"
resource = "secret:k3m4n5p6q7r8"
asks = "NPM_TOKEN"
```

The environment module keeps them, and the secret module names no
environment. The environment module reads your secrets as their holder,
and leaves out an approval whose secret is no longer kept.

<!-- docs-tree:start -->

## Docs

- [Your Yes](#your-yes): An environment you run acts as you, so it can be given the secrets you hold
- [Say Yes](#say-yes): A run at a terminal asks you once for each secret its file names with no approval yet, and keeps your yes
- [With No One There](#with-no-one-there): A run no one is there to answer, such as a script's, is refused a secret with no approval, naming the secret and the `lnk env approval add` command that gives it
- [Kept in Your Safe](#kept-in-your-safe): The approvals are in `~/.lnk/safe/envs/approvals.toml` (600), in your Safe, which no sandbox reaches

<!-- docs-tree:end -->
