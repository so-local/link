# What Is Embedded, and When

How Link Memory chooses which passages to embed next, how one thread
embeds for every agent a service serves, and how the vectors are kept.

## Embedding Queue

Embedding turns a passage's text into a vector, a list of numbers a
search by meaning compares, and on a computer's own processor it takes
far longer than indexing a passage's words. So every passage not
embedded yet waits in its index's queue, and memory embeds the front of
it first. A search by meaning covers what's embedded so far, a search by
words covers everything, and a search's reply says how much is embedded
(`embedded`), read from counters kept as passages are written.

### What Comes First

Conversations and notes come first, then documents whose text was
extracted from a PDF or a Word document, each newest first. A document
the agent reads moves up beside the newest, so what it's working with is
searched by meaning sooner. Triggers keep the queue as passages are
written and embedded, in the index's `queue` table (`src/queue.rs`).

### Picking a Batch

A batch of 16 is read from the queue's own index (`queue_order`), so
picking one reads 16 rows however many wait: the same with a thousand
passages waiting as with a hundred thousand
(`picking_a_batch_costs_the_same_however_many_wait`). Each passage is read
from its file as it's embedded. One whose file no longer holds what the
index says waits at the back until the index writes it again.

## One Thread for Every Agent

A service serves every agent of an owner, and one thread embeds for all
of them (`queue::Embedding`). It takes a batch from each agent's index in
turn, so one agent's large corpus doesn't hold up another's new note. An
index whose model fails waits 5 s before its next turn, and the others
go on. An index joins once its model answers, and leaves when it closes.

## How Vectors Are Kept

Each embedded passage keeps two forms of its vector, in two files beside
the index (`index.sqlite.codes` and `index.sqlite.small`), never in the
index and never loaded whole (`src/vectors.rs`). Both hold
`[tool.memory] dimensions` of the model's (Link's specification sets
256), so the target's 31 million passages take 1 GB of codes and 8 GB of
small vectors.

### Binary Codes

A binary code keeps one bit a dimension, its sign: 32 bytes a passage at
256 dimensions. A search by meaning compares its query's code with every
code by counting the bits that differ, from the codes' file mapped
rather than read, so its pages are the system's page cache, never a copy
in the service.

### Small Vectors

A small vector keeps one byte a dimension, with its scale. A search
reads only those of the 400 codes nearest its query's (`rescore` in
`src/limits.toml`), rescores them by their angle to the query, and
ranks each document by its best. Over 3,000 generated vectors of 1,024
dimensions, the true nearest is among the first ten in nearly every
query, and a search reads no more small vectors than it rescores
(`the_true_nearest_is_found_from_codes_and_small_vectors`).

### Slots

A passage's vectors sit at its slot, the same in both files. A slot let
go, as its passage changes or goes, is taken by the next passage
embedded, so the files never need rewriting whole. Each small vector
names its passage, so a slot taken again since a search began is never
read as another's. The files go with the index, in a move and in the
[copy kept off the machine](developing.md#streaming-it-back). Another model, another number of dimensions, or files missing
or short beside the index start them over, every passage embedded
again.

### Fewer Dimensions

A model trained to be cut (Matryoshka) keeps most of what it knows in its
first dimensions, so memory keeps those (`src/models.toml` lists the
models, the default `qwen3-embedding` among them). Any other model's
vectors are turned by a fixed random rotation first, so each dimension
kept holds an even share, and a search's reply says so.
