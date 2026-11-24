# Memory's Index

How Link Memory keeps each agent's index, writes it in the background,
and keeps it whole through a crash, a schema change or a full disk.

## What the Index Is

An agent's index is one SQLite file (`index.sqlite` in its state
folder) and the parts beside it, each named after it. SQLite is a
database kept in a single file, read and written by a library inside the
program, with no server. The index holds where each document's text is,
never the text itself: the agent's files stay the source of truth, and
the index is only ever what memory last read of them.

### Index Parts

Two parts sit beside the SQLite file. The words [search by
words](#search-by-words) reads are in a folder of their own,
`index.sqlite.words`, and the vectors [search by meaning
compares](embedding.md#how-vectors-are-kept) are in two files.

### What the Index Holds

The index holds three kinds of row, each written by the indexer and read
by every call.

- **Documents:** each one's path, kind, title, size, modification time,
  the hash of its content, and when it was first seen and last changed.
- **Passages:** each document's text split at its headings, then at
  blank lines, into pieces of up to 1,500 characters, short paragraphs
  packed together. Each keeps its byte range in the file, its first line
  and its hash. A passage is what a search ranks, what is embedded, and
  where a result points (`semantic::passages`).
- **Links**, the **removed** documents, and **counters** of the
  passages and of those embedded, kept by triggers as each passage is
  written.

### IDs Never Given Twice

A document or a passage never takes an ID one had before
(`AUTOINCREMENT`), since its words are named by it.

### What a Read Takes From the File

A `read` reads the file and slices its lines, a snippet reads its
passage's bytes alone (`source.rs`), and a read receipt names the file's
content as read. What memory makes itself and no file holds, a
document's extracted text and a conversation's copy made from its
events, is kept in the index's `derived` table and read from there. An
index of 12 generated files of 5,000 lines is under 0.6 times their size
(`the_index_is_a_fraction_of_the_corpus`), where one keeping the text
was 2.7 times.

### How an Edit Is Written

A file changed when its size or modification time differs from what's
indexed. Its passages are then compared by hash: one that didn't change
keeps its row, its words and its vector, moved to where it is now, and
only those new or changed are written. One line changed in a 5,000-line
file writes one passage again
(`a_one_line_edit_rewrites_one_passage`).

## Conversations

A conversation is indexed as it grows. The index keeps where its last
read ended (`since_kind` and `since`): the bytes of a copy kept as a
file, or the number of the last event in a copy memory makes from the
events, up to the agent's last answer.

### Reading What a File Grew By

The next read starts there (`Tree::read_since`). A file is read from its
last passage's start, a compact conversation's frames are decoded only
back to the one holding the last event read
(`link_store_contract::store::compact::read_from`), and a store
driver's or the storage bridge's conversation is asked for its events
from that one. The last passage is written again with what follows, and
the passages and links after it are added, the rest as it was
(`a_turn_appended_reads_only_what_it_added`).

### When the Index Starts Over

A copy that shrank, whose last passage no longer reads as it was, or
whose events no longer go on from the last one read, was written again:
it's indexed whole. A word from a 5 MB conversation's first turn is
found as one from its last
(`a_long_conversations_whole_history_is_indexed`).

## Search by Words

A search ranks passages, and a document ranks by its best passage, whose
first line is the result's `line`. Which passages hold each word is kept
in Tantivy, beside the SQLite file, so a search reads only the part of a
word's passages that can still reach its best, however many hold it.

### Tantivy

Tantivy is a search library written in Rust, widely used, which keeps
an inverted index: for each word, the passages holding it and how often,
in a folder of files (`index.sqlite.words`, `src/index/words.rs`). It's
memory's because SQLite's full-text search scores every passage holding
a word before it can rank one, so a common word's search grew with the
corpus, while Tantivy finds the best with block-max pruning, as [Why
Tantivy for Search by Words?](decisions.md#why-tantivy-for-search-by-words)
measures.

### Index Entries

Each passage is one entry, named by its ID and its document's, with its
words and no text, and each document's title is another, with the
title's words. Text is split at what isn't a letter or a digit,
lowercased, its accents dropped, and each word stemmed as English, so
"Planting" finds "plant".

### Block-Max Pruning

A word's passages are kept in blocks of 128, each with the best score a
passage in it can reach. A search keeps the best 50 documents so far,
and Tantivy skips every block that can't beat the last of them, unread.
So a common word's search scores about as many passages at ten times
the corpus (`a_common_words_search_reads_a_bounded_number_of_postings`).

### Every Word First

Passages holding every word of the query rank first, found by walking
the words' blocks together. When fewer than 50 documents hold every
word, passages holding any of the rarer words follow, ranked after them.
A word in more than a tenth of the passages (`rank-common`) is left out
of those when the query has rarer ones, so two common words never rank
every passage that holds either
(`every_word_first_then_the_rarer_words`).

### How a Passage Is Weighed

Tantivy weighs a passage by BM25, the measure search engines rank by:
each word counts by how rare it is among all passages, a word it holds
more often counts more, up to a point, and a shorter passage counts more
beside the average. A word in the document's title adds its rarity once
more, times `rank-title` in `src/limits.toml`, and a document only its
title matches ranks with the passages holding any word, from its first
passage.

### Search Deadline

A search stops ranking at its deadline (`[tool.memory] deadline`,
Link's specification sets `"300ms"`), looking at the clock with the
first passage it scores, every 256 after, and before each of Tantivy's
segments past the first, and answers with the best of what it ranked,
saying so (`stopped`,
`a_search_past_its_deadline_answers_with_what_it_has`). What follows the
ranking is a few rows a document shown, so the answer comes at the
deadline. A copy of a document shown is left out by its passages'
hashes, never by reading its text.

### Pages

A search's `next` names how far into what it ranked its last hit was,
and the next page passes over those before it without reading them. A
list's `next` names its last document by when it changed and its path,
and the next page is read from the index's order there
(`documents_changed`), never by counting past what came before.

### Counters

How many passages there are and how many are embedded are kept in the
index as each passage is written, by triggers. A search's reply reads
them, never counting rows (`the_status_is_read_without_counting_rows`).

## Indexer

One writer per index, a thread in the service, brings the index up to
the agent's folders in turns, as [one writer with many
readers](developing.md#one-writer-many-readers) does. A walk lists the folders and notes
what's new, changed (by size and modification time) or gone. Each turn
then writes one batch in a transaction of its own: up to 1,000 of
what's gone dropped (`removed-batch` in `src/limits.toml`), then what's
new or changed read, the newest first. Between turns a flush or a
write-back takes the index, and calls read it all along through
connections of their own.

### Bounded Batches

However much changed, no batch grows past its bounds: a folder of
thousands of files deleted at once is dropped a thousand at a time,
each in a short transaction (`what_is_gone_is_dropped_in_bounded_batches`).

### Batches Sized by Time

What a batch reads is held to about a second of work, reading and
committing (`batch-millis`), and to 8 MiB of text at most
(`batch-bytes`). The first batch reads one file, and each after reads
as much as the last one's speed says fits, so a slow machine writes
smaller batches than a fast one (`a_batch_is_sized_by_the_last_ones_speed`).
A batch also stops at the file it's on when a change is noted, so
[the change is written next](freshness.md#ahead-of-a-walks-batches).

### Indexer Threads

The turns of every index a service holds share a pool: as many at once
as the most indexing threads any agent's specification allows
(`[tool.memory] threads`, Link's specification sets 1), so a large
corpus being indexed leaves the machine usable.

### Write-Ahead Log

SQLite's write-ahead log (WAL) is a file beside the index where each
commit goes first, then is copied into the index at a checkpoint. Small
batches keep it small, and once checkpointed it's cut back to 256 MiB
(`wal-max-bytes`), so one large write never leaves it large.

### Page Cache

What the service holds for one agent's index is its specification's
`[tool.memory] memory` (Link's specification sets 512 MB), besides the
pages of files it maps: the writer's connection caches half of it,
Tantivy's writer buffers a quarter of words before it writes them as a
segment, and each session's reader a sixteenth.

## Crash and Rebuild

Every committed batch stays committed. A crash, a kill or the service
ending loses at most the batch it was writing: the next start's walk
finds what's indexed by its size and time, and reads only the rest
(`a_build_killed_midway_resumes_from_what_was_committed`). The service
keeps running while any of its indexes has a batch left, so what it
began is finished rather than begun again.

### Words and SQLite in Agreement

A batch commits twice, SQLite first and then Tantivy, and Tantivy's
commit names the batch's generation, a count SQLite raises in the same
transaction (`meta.words`). A crash between the two leaves Tantivy a
batch behind, and the next start finds it, the two generations
differing.

### Setting the Words Right

The writer then sets the words right by the passages before
its first batch: every title written again, a passage the words hold
that SQLite doesn't dropped, and one SQLite holds that the words don't
dropped too, its document read again whole. A file the lost batch wrote
is found, one it removed isn't, and the two hold the same passages
(`a_crash_between_the_two_commits_leaves_them_agreeing_at_the_next_start`).
The same mends words that didn't come with a copy, or came from
another moment than its SQLite file.

### One Writer at a Time

Tantivy lets one writer at a time hold a folder, by a lock in it. An
index's writer takes it for a batch and lets it go once nothing is left
to write, its merges finished, so another process serving the same file
writes in turn. Meanwhile the other's indexing waits, and a search's
reply says so (`indexing.paused`). A batch reads each document as the
index holds it when it writes it, so one the other process wrote since
this one's walk is written over, or left as it is when it's current
(`another_writer_of_the_same_file_writes_in_turn`).

### Another Schema

An index of another schema is moved aside, with its log and its
words, and deleted in the background, and a new one is built in its place: a start never
waits while an old, large index is dropped table by table
(`an_older_schema_opens_without_waiting`).

## Disk Use

Before each batch the indexer checks the disk the index is on keeps
free what the agent's specification says (`[tool.memory] reserve`,
Link's specification sets 1 GB). Indexing then waits, calls go on
answering from what's indexed, and a search's reply says why
(`indexing.paused`). It resumes at the writer's next turn once the disk
has room (`indexing_pauses_while_the_disk_is_short_of_its_reserve`). A
paused index holds up no service's end.

### Full Disk

A commit that finds the disk full, SQLite's or Tantivy's, pauses
indexing the same way for a minute (`full-retry-secs` in
`src/limits.toml`). Meanwhile the writer reads and writes nothing for
it, not even [setting the words right](#setting-the-words-right) after
a commit Tantivy lost, then tries again
(`a_full_disk_pauses_without_setting_the_words_right`).
