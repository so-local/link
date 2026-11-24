# Format

`link-format` renders everything `lnk` prints, as typed blocks that one formatter draws to a terminal, to a pipe, and as JSON. A command writes blocks and returns how it ended, and one file decides how the blocks look.

<!-- docs-tree:start -->

## Docs

### Pages

- [Format](docs/more/libraries/runtime/format.md): `link-format` is everything `lnk` prints, as typed blocks one formatter renders: styled to a terminal, plain to a pipe or a file, and as `--json`
- [Exit Codes](docs/more/modules/exit-codes.md): What each code `lnk` and its modules exit with means, so a script can tell a refusal from a failure

### More

- [Format Decisions](docs/more/libraries/runtime/format/decisions.md): Why `link-format` looks and behaves as it does: what its colors, marks, words and layout mean, and why each is kept to so few

<!-- docs-tree:end -->

## Code

- [`block.rs`](src/block.rs): the blocks
- [`lib.rs`](src/lib.rs): `run`, and the types a command uses
- [`look.rs`](src/look.rs): the look and output's settings
- [`out.rs`](src/out.rs): `Out`, where a command writes its blocks
- [`render.rs`](src/render.rs): each block as text
