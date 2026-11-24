# Box Module

The box module, `lnk-box`, makes machines in your own cloud accounts that
run `lnk`. It makes a box, reaches it over SSH, and removes it. The modules
that run anywhere also run on a box.

<!-- docs-tree:start -->

## Docs

### Pages

- [Boxes](docs/services/boxes/README.md): Create and manage computers in your own cloud account, with Local Link installed
  - [Boxes Guide](docs/services/boxes/guide.md): A computer in your own cloud, running `lnk`
  - [Saved Images](docs/services/boxes/image/README.md): Save a configured box as an image, then reuse its disk in new boxes in the same cloud account
  - [Machine Sizes](docs/services/boxes/size/README.md): Preview the machines that match a box specification before you create a box
  - [Box Users](docs/services/boxes/user/README.md): Give people Linux logins on a box, each confined by a sandbox specification
  - [Box Life Cycle](docs/services/boxes/lifecycle/README.md): A box lives from `lnk box create` until `lnk box delete`, and this page follows it through each stage
  - [Reach a Box](docs/services/boxes/ssh.md): `lnk box ssh` opens a shell on a box, or runs one command there, over OpenSSH with Local Link's own key and a certificate that lasts an hour
- [Box Specification Guide](docs/config/specs/guides/box.md): How Link reads a box specification, what its values mean and how it picks the machine

### References

- [Box Specification](docs/config/specs/box.md): A box specification describes a machine in your own cloud as a TOML file: its CPUs, memory, disk, system and region, the `lnk` installed on it, what it attaches, and how a box made here is isolated
- [Box Commands](docs/more/reference/modules/compute/box.md): Every command the `box` module offers, from its declarations, in order of the resource each acts on

### Packages

- [Box Contract](contract/README.md): The box contract describes the machine a workload addresses, whichever cloud provider runs it

### More

- [More](docs/services/boxes/more/README.md): An adapter implements the operations behind the box commands
- [Decisions](docs/services/boxes/decisions.md): Why clouds and boxes work the way they do
- [Security](docs/services/boxes/security.md): What's protected: your cloud accounts, whose keys `lnk cloud` keeps, and the machines `lnk box` makes in them, against anyone but you reaching them
- [Troubleshooting](docs/services/boxes/troubleshooting.md): What to do when a box won't make, start, show or save, symptom by symptom

<!-- docs-tree:end -->

## Code

- [`main.rs`](src/main.rs): `lnk box`'s commands
- [`setup.rs`](src/setup.rs): a new box's first boot and install
- [`spec.rs`](src/spec.rs): the box specification
- [`ssh.rs`](src/ssh.rs): reaching a box
- [`users.rs`](src/users.rs): a box's users
