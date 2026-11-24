# Read PDFs and Word Documents

> Documents Become Searchable Text

Link Document Converter reads the text of PDFs and Word documents for your
agent's memory. It's a module of its own, off until you add it, so a
Link that doesn't need it never installs a document parser.

```sh
lnk mod add document-converter
```

With it, memory finds a PDF (`.pdf`) or a Word document (`.docx`) in
`memory/` by the text inside it, up to 32 MiB and the first 2 MiB of
its text. Without it, memory finds one by its name alone, and a
search's reply names the module that reads it. Once you add it, your
agent's next start reads every document memory found by its name for
want of it.

## Converter Contract

The [converter contract](../../../../contract/docs/agents/tools/memory/documents/more/contract.md) defines formats, size limits, and the typed answer that Memory reads from the child process.

## Parser Boundary

[Developing](documents/more/developing.md) explains parser dependencies and the child process’s resource limits, including how an extraction failure leaves the rest of memory available.
