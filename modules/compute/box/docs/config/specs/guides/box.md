# Box Specification Guide

How Link reads a box specification, what its values mean and how it picks the machine. Each field is in the [Box Specification](../box.md) reference.

```toml
schema = 1
name = "web"
version = 3

[machine]
cpus = { min = 2, max = 8 }
memory = { min = "4 GB", max = "16 GB" }
disk = { min = "40 GB", type = "balanced" }
tags = { team = "research" }

[machine.aws]
region = "eu-west-2"
size = "t3.large"

[machine.gcp]
region = "europe-west2"
memory = { min = "8 GB", max = "32 GB" }
spot = true
```

## Read a Specification

Every key is optional. Left out, a key is the least that works, and an
empty file makes the box `lnk box create <cloud>` makes with no
flags. A box's setup isn't a key: each stage is a [Sandbox
Specification](../../../../../../security/sandbox/docs/config/specs/sandbox.md)
your setup script runs on the box, as [Set a box up in
stages](../../../services/boxes/guide.md#set-a-box-up-in-stages) shows.

### Files and Flags

`lnk box create`, `lnk box size list`, `lnk box check`, and `lnk box
spec show` take the file as `--spec FILE` and each field as
a flag spelled as its key path, such as `--machine.cpus 4` or
`--machine.aws.region eu-west-2`. A flag sets its key alone, as the
file's line would. A file is at most 64 KB. An unknown key, an empty
table, or a value that isn't one is refused when the file is read, the
error naming its line, and an unknown key by its name, in `[machine]`
too, where every other table is a cloud's.

### A Cloud's Own Table

A cloud's table, `[machine.<cloud>]`, is named by the cloud's kind, as
[`lnk cloud list`](../../../../../../cloud/cloud/docs/services/cloud/guide.md#connect-a-cloud-account)
says it: `[machine.aws]`, `[machine.gcp]`, or another adapter's. It
takes the same keys as `[machine]` and replaces them on that cloud, each
key whole, so `[machine.gcp] memory` above has no say from
`[machine]`'s. `region` and `size` go only there, since each names one
cloud's, and a cloud's table can't hold another table. A table for a
cloud the box isn't on has no say.

#### Which Wins

The cloud's table, then `[machine]`, then Link's default. A flag sets
the key it names, so `--machine.cpus 4` changes `[machine]` and
`--machine.aws.cpus 4` changes AWS's table alone. `--zone` with
`--cloud` is the region of that cloud's table. `lnk box create` notes
a `[machine]` flag the cloud's table outranks: its own key, or its
`size`, which has its cpus, memory and GPUs built in.

### Isolation Here

A box made `here` is isolated as [`lnk agent create`](../../../../../../agents/agent/docs/agents/start/create.md#create-on-a-hosting-account)'s
agents are: in Link's sandbox, the default, in the sandbox specification
`isolation.sandbox` names, read once when the box is made, or directly
with `isolation.no_sandbox`, refused where this machine's agent policy
says `unsandboxed = false`. A cloud account's box is a machine of its
own, so it refuses the whole table before anything is made.

### Environment

`env` names an environment file for every placement. Creation captures
the checked file and its declared inputs once, before anything is
made. A box here sets it up on this machine, as [`lnk env setup here`](../../../../../env/docs/services/environments/guide.md#set-a-machine-up-without-an-image)
does. A cloud box starts from the environment's image when it saves
stages, then runs its start stages and services on the machine. Each
box keeps its captured environment and starts it again at `lnk box
start`, never reading its source again, as the [machine
contract](../../../../contract/docs/services/boxes/more/contract.md) describes.

### The `lnk` on the Box

From a file, `[lnk] version` can't be behind this computer's `lnk`, so
a specification someone hands you can't install a release with a known hole. To
mean a lower one, type `--lnk.version` yourself.

## Values

What each `[machine]` key takes, and what the cloud makes of it.

### A Count or a Range

A count or size alone means exactly that, as in `cpus = 4`. A range is
`{ min = 2, max = 8 }` with either end left out, but not both, and
`min` never over `max`. On the command line each end is its own flag,
`--machine.cpus.min 2 --machine.cpus.max 8`. A count is at most 4096.

### A Size

Sizes are strings with their unit, as `"8 GB"`, `"512 MB"` or `"1
TB"`, and a bare number is refused. GB means GiB, as both clouds count
memory. On the command line it is `8GB`.

### GPUs

`gpus = 1`, or `gpus = { min = 1, max = 2, model = "nvidia-l4" }`. A `model` alone means at least one. Models are
`nvidia-t4`, `nvidia-l4`, `nvidia-l40s`, `nvidia-a10g`, `nvidia-a100`,
`nvidia-h100`, `nvidia-h200` and `nvidia-b200`.

### arm64

`arch = "arm64"` gets Ubuntu's arm64 image. The machine type is matched
among the cloud's arm64 ones as `cpus` and `memory` are, since the
default size is x86_64, or it is the arm64 `size` given. It has no
GPUs, because NVIDIA's images are x86_64. A `size` its cloud lists as
the other processor, such as `t4g.small` on an x86_64 box, is refused.

### The Disk

`disk.min` is what the box gets, unless its image needs more. NVIDIA's
images for a box with GPUs are larger than 20 GB, so such a box gets
its image's size, said as it's made. `disk.type` is `balanced` or
`fast`, and a fast disk is billed for its speed too.

### Spot

`spot = true` is cheaper, but the cloud may take the machine back. It
stops the box and never deletes it, so its disk stays and is billed.
`lnk box list` says the cloud stopped it.

#### Telling the Cloud Stopped It

AWS says so in the instance's state reason. Google Cloud doesn't, so
there it is a spot box found stopped that ran when this computer last
saw it, which `lnk box stop` on another computer looks like too, and
the list says both.

#### Starting Again

On AWS it is a persistent spot request, which starts the box again on
its own once AWS has the capacity. `lnk box delete` cancels the request
first, and cleanup cancels Link's requests whose box is gone. On Google
Cloud, `lnk box start <box>` starts it.

### Tags

`tags` keys start with a lowercase letter, and keys and values are
lowercase letters, digits and dashes, at most 63 each. A box takes at
most 40 of yours in all. Keys starting `lnk-` or `aws:`, and `Name`,
are Link's or the cloud's, and refused. On Google Cloud they're labels.
Each tag is its own flag, `--machine.tags.team research`.

### A Region

`region` on Google Cloud is a region or a zone in it. A region means
its first zone that's up.

## Deadline

`deadline = "4h"` limits a box's lifetime, from one second through
seven days, and `lnk box list` shows it, its JSON too.

### How Each Cloud Ends It

Google Cloud sets the instance's maximum run duration and deletes it at
termination. AWS terminates on shutdown and installs a root-owned
systemd timer at first boot to power off at the deadline. A process
running as root can change that timer.

### In an Image

`lnk box image create` leaves the timer out of the image, so a box made
from it never ends at the deadline of the box it was saved from.

## How the Machine Is Picked

From the specification's CPUs, memory and GPUs, or its `size`, among what the
cloud has where the box is made (`lnk box size list`).

### The Default Size

With no `cpus`, `memory` or `gpus`, the box gets the account's default
size: `t3.small` on AWS, `e2-small` on Google Cloud.

### The Smallest That Fits

Otherwise it gets the smallest that fits, by CPUs, then memory, then
GPUs. Memory is at least 2 GB when the specification says none, and a box has no
GPU unless it asks. Between equals it takes the cheaper when the cloud
gives a price, then the cloud's order: general purpose first, current
generation, cheapest family first.

### Room for an Agent's Memory

An agent's memory searches by meaning with an embedding model, a model
that turns text into numbers so notes that mean the same thing are found
together. That model runs on the box, beside the agent, and needs memory
of its own. Memory's default, `qwen3-embedding`, is Ollama's 8-billion
parameter model, about 4.7 GB. It won't fit the default size, an
`e2-small` on Google Cloud or a `t3.small` on AWS, each with 2 GB. Give a box that runs it at least
8 GB, such as an `e2-standard-2` or a `t3.large`, or have the agent
search by words alone with `[tool.memory] model = "none"` in its specification.

```sh
lnk box create gcp --name gcp-1 --machine.memory 8GB
```

### A Machine Type

`size` wins. A machine type has its CPUs, memory and GPUs built in, so
a table giving `size` and any of those is refused. `arch` with `size`
must be its processor. A size the adapter doesn't list there, such as
an older generation or a GPU Link has no name for, is made as given,
with a warning, and the cloud says whether it has it.

### When Nothing Fits

The error names the nearest on each side: "the smallest with 4 CPUs
has 16 GB, over your max of 8 GB".

### GPUs on Google Cloud

A box's GPUs are those built into its machine type (G2, A2, A3, A4), or
T4s attached to an N1 machine. Link names an N1 machine with T4s
`<machine type>.nvidia-tesla-t4.<count>`, such as
`n1-standard-4.nvidia-tesla-t4.1`, and `size` takes that name too. A
T4 box is made only in zones that have T4s.

### NVIDIA's Driver

A box with GPUs gets Ubuntu with NVIDIA's driver: AWS's Deep Learning
Base AMI, or Canonical's accelerator image on Google Cloud. Both clouds
give a new account no GPUs until you ask them to raise its quota, and
`lnk box create` says where when the cloud refuses.

### Plan for a Cloud

`lnk box check --plan <kind>` prints what a specification asks of a box in a
cloud of that kind instead of looking anything up, its table for that cloud laid over and every default filled
in: the request a job carries to the box that runs it.

```sh
lnk box check --plan aws --spec web.toml
```

## What `lnk box spec show` Prints

`lnk box spec show <box>` prints the specification the box was made from, as
this computer kept it in `boxes.toml`, with any field flags laid over.
`--json` prints the same fields as JSON. Why it works this way:
[Decisions](../../../services/boxes/decisions.md).

### What Its Cloud Pins

Its cloud's table pins what the cloud says the box is: its `size` and
`region`, and its `disk`, `os` and `spot` where they differ.

### A Box From Before, or Elsewhere

A box made before specifications, or on another computer, prints only what the
cloud keeps, and its `[lnk]` is left to the defaults. An `[lnk]
version` behind this computer's `lnk` is left out too, so the file
reads back, and said on stderr with the `--lnk.version` that makes it
again.

## Attachments

`[attach]` names existing resources the machine uses without creating
or deleting them, and its own boot disk remains disposable. Removal
keeps attached disks and addresses, and Link never formats a disk. An
AWS data volume also requires its subnet, in the same availability
zone.

### Check Attachments First

`lnk box check` looks them up without making anything and says they're
present, or with `--json` prints `{"kind": "attachments", "attachments": "present"}`. The
cloud refuses missing resources before it launches the machine.

```sh
lnk box check gcp --spec service.toml
```

<!-- spec-reference: box -->
