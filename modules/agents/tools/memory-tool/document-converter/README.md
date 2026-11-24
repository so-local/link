# Link Document Converter

Link Document Converter (`lnk-document-converter`) is a module that reads the
text of PDFs and Word documents for Link Memory's index.

<!-- docs-tree:start -->

## Docs

### Pages

- [Read PDFs and Word Documents](docs/agents/tools/memory/documents.md): Link Document Converter reads the text of PDFs and Word documents for your agent's memory

### Packages

- [Document Converter Contract](contract/README.md): `link-document-converter-contract` holds what Link Document Converter answers, the text of a PDF or a Word document, for Link Memory to read

### More

- [Developing](docs/agents/tools/memory/documents/more/developing.md): The document converter runs parsers for Memory in a separate process with bounded input and output

<!-- docs-tree:end -->

## Code

- [`main.rs`](src/main.rs): the program and its parsers
