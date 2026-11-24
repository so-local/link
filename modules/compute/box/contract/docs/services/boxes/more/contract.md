# Machine Inputs

The box contract describes the machine a workload addresses, whichever provider runs it, with its captured inputs, creation records, guest bootstrap and authenticated access.

## Machine providers

A provider is the program that runs a box's machine. Box makes cloud
machines through its cloud adapters, and places a box here on this
computer itself. A machine provider is a module that runs a box's
machine on this computer in a way of its own, and the types on this
page are the seam it meets Box at.

### Finding a provider

A provider is the installed module whose manifest answers Box's `box.machine` question (`QUESTION`), as [Questions](../../../../../../../../core/framework/docs/more/modules/protocol.md#questions) describe. Box finds it by the question and never by its name, so Box builds, installs and runs with no provider program. With none installed, no name is a provider's box, none is listed, and any other provider operation is refused. Two modules answering the question are refused rather than either chosen.

### What the provider answers

Box asks the provider whether a name is its box (`Lookup`), for its inventory (`List`) and for the machine owner's policy (`Policy`), and never reads the provider's files. A login's `SshAccess` carries the `lnk` command words that carry its SSH connection, and the path of an approved filtering proxy's socket when the machine has one.

### Records Box carries

A provider keeps its own request, plan, launch handle, configuration and input specification in Box's records. Each record is generic over a `Provider` that names those types. Box uses `Opaque`, which carries each payload as the JSON value the provider wrote and never reads inside it. A provider names its own typed request, plan and handle.

### One saved JSON

Both views read the same saved JSON, so records written before a provider changed hands stay readable. Box may write a payload's object keys in another order, never another value. `convert` reads one view as the other and refuses JSON that doesn't fit the target's types, which is where a provider validates what Box carried.

### Shared machine types

The types every provider's machines share live here, and a provider's contract uses them rather than a copy. `Lease` says how long a machine may live, and `Intent` names the lifecycle asked of it. `GuestTarget` names its operating system, processor and release. `SshPublicKey` carries the public half of a login.

### Provider descriptions

A provider's replies describe each machine with a `MachineSummary` of its target, login user, place and size. Box lists and reaches a box from that summary. The provider fills it on every reply and never saves it, so it always reflects the provider's current record.

## Captured inputs

`MachineInputs` carries an optional input of the provider's own and an optional environment bundle. Each input records its content identity and whether it came from the caller, a saved reference or the machine owner's policy. The provider reads its own input once and returns what it captured, so Box never parses one. Environment files travel as captured content. Their original paths explain provenance and never instruct another machine to read the caller's filesystem.

## Creation binding

`PendingBox` records creation intent before resolution. `MachineBinding` records the provider's resolved request, plan, policy revision, lease, launch generation and control revision before execution. `Lifetime` distinguishes a guest created for one run, a borrowed machine and a deliberately retained box. The machine provider owns execution selection, resource admission, and confirmed removal.

## Access and bootstrap

`SshAccess` names caller-held identity files, a pinned guest host alias and the provider's connection command. Private key bytes remain local. `GuestBootstrap` declares target-compatible program artifacts by digest and size. Machine readiness, authenticated SSH, Link availability, sandbox readiness and environment preparation remain separate observations.

### A socket forwarded over SSH

`forward` gives another machine a Unix socket with `ssh -R` (SSH's
remote forward, which listens on the far machine and carries each
connection back), as [`lnk box forward --socket`](../../../../../docs/services/boxes/ssh.md#forward-home-to-a-box) does.

#### Preparing the machine

`socket_forward_prepare` is what the machine runs over SSH first: the
socket's folder made in its runtime folder (`$XDG_RUNTIME_DIR/lnk`, or
`/tmp/lnk-run-<uid>` without one), only its user's; a socket a
connection before left there removed, since sshd binds over none and
removes none; and the folder printed, which `ssh -R` needs whole. It's
one line with no `'`, `!` or `\`, so any login shell passes it to `sh`
as it is. `socket_forward` is `ssh -R`'s forward to the socket, from a
port on this machine's loopback.

## Local protocol

`HereCall` and `HereReply` each state the version of the machine protocol they speak, which the contract's `protocol.toml` declares (`MACHINE_PROTOCOL`, now 1), and each side [refuses a peer older than its `oldest`](../../../../../../../../packages/version/docs/more/libraries/types/version.md#protocol-versions) by name, naming [`lnk upgrade`](../../../../../../../../docs/more/modules/guide.md#upgrade).

### Its messages

They use closed operation enums and unknown-field refusal. Each message is one bounded JSON exchange with the provider's process. The box bridge reserves standard output for its JSON reply and sends nested progress to standard error. The provider implements creation, inventory, access, bootstrap and lifetime controls. This compute-layer protocol keeps the generic cloud contract free of any provider's dependencies.
