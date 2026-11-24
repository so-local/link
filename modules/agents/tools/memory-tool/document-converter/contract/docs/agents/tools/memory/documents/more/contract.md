# Document Converter Contract

`link-document-converter-contract` holds what Link Document Converter,
`lnk-document-converter`, takes and answers, generated from the module's
declarations (`actions.toml` and `types.toml`): the text of a PDF or a
Word document, [for Link Memory to read](../../../../../../../docs/agents/tools/memory/documents.md).
Its code is in [`link-document-converter-contract`](../../../../../../README.md).

## `lnk-document-converter text show <pdf|docx>`

Its caller builds the line with `text_argv`, runs it as a child with the
document on its stdin, and reads one `Extracted` on its stdout, as JSON:
`{"text": "..."}`, the document's text, or `{"unread": "..."}`, why it
has none. Any other key is refused. The program reads that line with its
own parser, generated from the same declarations.

## Its formats

`Format` is declared as the two formats it reads: `Format::Pdf` is named
`pdf` and `Format::Docx` `docx`, on the command line and as the extension
a document's name is known by, in any case
(`Taxes 2025.PDF`). A name with any other extension, `.doc` among them,
is read as text.

## Its limits

`limits()` gives them, read once from the contract's `src/limits.toml`,
so the converter and Link Memory, which runs it, hold to the same ones.
`document-max-bytes` is the largest document read, 32 MiB, a long
report, scanned pages aside. `text-max-bytes` is the most text kept of
one, 2 MiB: the rest isn't indexed.

## The module

`MODULE` is the module's name, `document-converter`, whose program is
`lnk-document-converter`, the one the memory module starts for its documents.
