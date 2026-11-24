# Document Converter Contract

`link-document-converter-contract` holds what Link Document Converter answers, the text of a PDF or a Word document, for Link Memory to read.

<!-- docs-tree:start -->

## Docs

- [Document Converter Contract](docs/agents/tools/memory/documents/more/contract.md): `link-document-converter-contract` holds what Link Document Converter, `lnk-document-converter`, takes and answers, generated from the module's declarations (`actions.toml` and `types.toml`): the text of a PDF or a Word document, for Link…

<!-- docs-tree:end -->

## Code

- [`build.rs`](build.rs): generates the code `cli.rs` includes, from the declarations
- [`cli.rs`](src/cli.rs): its shapes, runtime arguments and their command line, generated from the module's declarations
- [`lib.rs`](src/lib.rs): its formats, limits and answer
