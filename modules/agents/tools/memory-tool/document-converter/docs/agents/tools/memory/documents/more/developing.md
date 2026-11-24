# Developing

The document converter runs parsers for Memory in a separate process with bounded input and output. Use this page to inspect the process boundary and the parser dependencies.

## How It Runs

Memory runs `lnk-document-converter text show <pdf|docx>` for each document, the
document on its input and the text, or why there's none, on its output.
It runs as a child of memory that holds none of memory's secrets and no
environment, requesting operating-system memory and CPU limits; one that crashes
or takes over 30 seconds fails that document alone. Memory finds it
beside its own program, and its sandbox reads it where it's installed.

## Resource Limits

Memory clears the child’s environment, bounds its answer, and kills it
after a 30-second wall-clock deadline. It also requests a 1 GiB address
space limit and CPU limits from the operating system. A platform that
does not enforce a requested limit, including the memory limit on macOS,
still has the separate deadline. The deadline and the memory limit are
memory's, `extract-deadline-secs` and `extract-memory-max-bytes` in its
`src/limits.toml`.

## Its Parsers

PDFs are read with `lopdf`, and Word documents with Link's own reader of
their text part, the zip unpacked with `zip`. An image-only PDF has no text layer to
extract; the converter performs no optical character recognition.
