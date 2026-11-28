# Troubleshooting

What to do when [your agent's memory](README.md) isn't served, doesn't find a file
or searches by words only. Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk agent show
```

## "Your agent's memory isn't served" at a start

Run [`lnk agent memory start`](../../../../../../agent/docs/agents/state/files.md#lnksafesandboxestoolsmemory-idjson-600) again: it says what failed, and serves
your agent's memory once that's fixed.

## `$LNK_MEMORY` is empty in `lnk agent harness run`

[`lnk agent memory`](../../../../../../agent/docs/agents/state/files.md#lnksafesandboxestoolsmemory-idjson-600) says why: your agent has no memory, or it runs with
no proxy to reach it through.

## A file isn't found

It isn't in `memory/`, or it's larger than `[tool.memory] file` (16 MB unless you
chose otherwise; a PDF or Word document, 32
MiB), hidden, a link, or not a file it reads.

## A file just written isn't found

On macOS memory finds a change by walking the folder each minute: search
again within a minute. On Linux a change is found within a couple of
seconds, unless the machine's limit on watched folders is reached,
which a search's reply says, and then within a minute too. While memory
reads a large folder for the first time, a search's reply says so and
how many changes wait (`indexing`), and a file written meanwhile is
indexed by the next batch.

## "Indexing is paused"

A search's reply gives the reason (`indexing.paused`), and calls go on
answering from what's indexed. The disk has less free than
`[tool.memory] reserve` (1 GB unless you chose otherwise), or a commit
found it full: free some room, and indexing resumes at its next turn, or
within a minute of a full disk. Another process writes the same index:
indexing resumes once that process has nothing left to write.

## "found by its name only"

The PDF or document has no text to read (a scan), the module that
reads it isn't added, which [`lnk mod add document-converter`](../../../../../../../../docs/more/modules/guide.md#add-or-remove-a-module) adds, or its parser
failed, crashed or took over 30 seconds: the reply says which.

## "Search is by words only"

The embedding model isn't there or isn't running. Install it with `lnk
model install qwen3-embedding`, or start Ollama (`lnk model list
--start`). A model still starting is used once it answers, with no
restart. An index kept off the machine past `[tool.memory] kept_off`
(2 GB unless you chose otherwise) is searched by words, as its reply
says.

## A search says it stopped

A search ranks by words until `[tool.memory] deadline` (300 ms unless
you chose otherwise), then answers with the best of what it ranked
(`stopped`). Add words to the search, or raise the deadline in the
agent's specification.

## It has no `remember`

Its sandbox file keeps `instructions/` read only, so it can't write its
notes, which [`lnk agent show --permissions`](../../../../../../agent/docs/agents/models/agent/README.md#set-its-model-and-how-hard-it-thinks) shows.
