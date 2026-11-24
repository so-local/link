# Environment Fixture

`link-env-fixture` gives [environment](../../../modules/compute/env/README.md)
and job tests a folder-backed cloud,
fake programs and inspectable results. It runs real Link commands against
stand-ins, without a cloud account or real sandbox enforcement.

## Code

- [`lib.rs`](src/lib.rs): start at `World::new` for owned setup, then follow commands, results and assertions
- [`standins.rs`](src/standins.rs): the environment specification and fake controller, box, job cloud and storage programs
