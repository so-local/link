# Archive Module

The archive module, `lnk archive`, exports your things' state to one
`.tar.gz` file and imports one. It only reads and writes files, using the
[archive package](../../../packages/archive/README.md).

<!-- docs-tree:start -->

## Docs

### Pages

- [Archives](docs/tooling/archive/README.md): Archives carry agent state between machines and people in one file

### References

- [Archive Commands](docs/more/reference/modules/tooling/archive.md): Every command the `archive` module offers, from its declarations, in order of the resource each acts on

### Packages

- [Archive Questions](contract/README.md): `link-archive-contract` holds the questions the archive module asks the modules whose kinds it carries

<!-- docs-tree:end -->

## Code

- [`checks.rs`](src/checks.rs): the hooks its declarations name
- [`cli.rs`](src/cli.rs): its command line and outputs, generated from `actions.toml` and `types.toml`
- [`main.rs`](src/main.rs): export and import, and what they print
- [`owners.rs`](src/owners.rs): each kind's owner, asked through its module's answers
