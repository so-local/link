# Environment Module

The env module, `lnk-env`, sets up machines from reviewed environment
files. Each stage of a file runs in its own sandbox, with the variables,
secrets and services the file names, on a box or on the machine you type
on.

<!-- docs-tree:start -->

## Docs

### Pages

- [Environments](docs/services/environments/README.md): An environment is one file that says what a machine has before work starts, so every box made from it is set up the same way
  - [Environments Guide](docs/services/environments/guide.md): Write an environment file, check it, then put it on a machine: a new box, a box you have, or the computer you type on
  - [Environment Life Cycle](docs/services/environments/lifecycle.md): An environment is a file you keep, `env.toml`, and Local Link keeps a few records about it
  - [Environment Approvals](docs/services/environments/approval/README.md): Manage the permission an environment file needs to receive secrets you hold
  - [Environment Images](docs/services/environments/image/README.md): An environment image is a box's disk with the environment's setup stages already run, saved in your cloud account

### References

- [Env Specification](docs/config/specs/env.md): An environment specification says what a machine has before work starts: its OS, where work starts, the values every run is given, the stages that set it up and the image they make, and the services it runs
- [Env Commands](docs/more/reference/modules/compute/env.md): Every command the `env` module offers, from its declarations, in order of the resource each acts on

### Packages

- [Environment Contract](contract/README.md): `link-env-contract` is the package that reads the environment file as strictly as every specification is read

### More

- [Interface](docs/services/environments/more/protocol.md): The environment module exposes process commands for running setup and services on another machine
- [Decisions](docs/services/environments/decisions.md): Why environments work the way they do
- [Security](docs/services/environments/security.md): An environment file determines setup, services and the values they receive
- [Troubleshooting](docs/services/environments/troubleshooting.md): An environment failure names the file line, stage or service involved

<!-- docs-tree:end -->

## Code

- [`carried.rs`](src/carried.rs): an environment prepared in a workdir, and carried
- [`here.rs`](src/here.rs): `lnk env here`, on the machine
- [`main.rs`](src/main.rs): `lnk env`'s commands
