# Bundle Module

The bundle module, `lnk bundle`, builds any set of Local Link's programs
from source for their targets. It puts them into one signed folder in the
published release layout.

<!-- docs-tree:start -->

## Docs

### Pages

- [Bundle](docs/tooling/bundle/README.md): `lnk bundle` builds any set of Local Link's programs from source into one signed folder that every installer already reads
  - [Signers](docs/tooling/bundle/signers/README.md): A signer is what signs a bundle

### References

- [Bundle Specification](docs/config/specs/bundle.md): Which of Link's programs to build from source, for which targets, and how to seal and sign them into one release folder
- [Bundle Commands](docs/more/reference/modules/tooling/bundle.md): Every command the `bundle` module offers, from its declarations, in order of the resource each acts on

<!-- docs-tree:end -->

## Code

- [`cli.rs`](src/cli.rs): command parser, one type per command
- [`main.rs`](src/main.rs): the commands' output
- [`make.rs`](src/make.rs): `create`, end to end
- [`seal.rs`](src/seal.rs): sealing, signing and checking sums
- [`show.rs`](src/show.rs): what a bundle holds, checked
