# How Memory Stays Current

How Link Memory learns that a file changed without walking the agent's
folder to answer a call.

## Change Journal

Memory never walks its folder to answer a call. Each agent's index
keeps a journal of paths that may have changed (`index::Journal`), and
the index's writer drains it: each path asked of the folder on its own,
indexed if it changed and dropped if it's gone. A call answers from
what's indexed, so its cost depends on what it asks, never on the
folder's size, and while paths wait a search's reply says how many
(`indexing.waiting`). `calls_never_list_and_a_file_written_is_found_soon`
makes a hundred calls and sees no listing, then writes a file and finds
it by the next search within 2 s.

### Folders on This Machine

On Linux memory watches the folder with inotify, the kernel's notice of
what changes in a folder: a watch on `memory/`, on `chats/` and on each
folder in them, set when the index is served. A change is noted in the
journal at once, and the writer woken for it. A change to any file in a
conversation's folder, its events or its frames, is noted as its copy,
`chats/<thread>/conversation.md`.

### Folders That Come, Go or Move

A folder that appears is watched, and what's in it found by a walk. A
folder renamed inside the tree keeps its watch under its new name, and
one moved out of it is no longer watched
(`a_folder_renamed_stays_watched`).

### Trees Through the Files Bridge

A Home kept in a bucket is reached through the files bridge, which
sends memory a notice naming each path it changes
(`Control::Changed`). The path goes in the journal, and memory lists
the bucket once the changes settle, 2 s after the last, so a burst of
writes is listed once.

### Conversations a Store Keeps

A conversation kept by a store driver, or reached through the storage
bridge, changes where no watch here sees. The reconciling walk asks the
store for each conversation's last event number, in one call, and one
past what's indexed is [read from the event after it](index.md#conversations).

### Ahead of a Walk's Batches

While the writer works through what a walk found, the first build or a
reconciling walk, each turn first moves the paths the journal noted to
the front of what waits, the newest first, and writes them in a batch
of their own. A file written while a large folder is being indexed is
written by the next batch, not after the rest
(`a_file_written_during_a_build_is_written_next`).

### Batch Under Way

The batch being written when a change is noted doesn't hold it up for
long: a batch is [held to about a second of
work](index.md#batches-sized-by-time), and one reading files commits at
the file it's on. A file written while a batch is being written is
found within 2 s (`a_file_written_during_a_long_batch_is_found_soon`).

### Paths Not Reached

A path the folder can't answer for now, its file system busy or a
bridge between answers, stays in the journal and is tried again at the
writer's next turn, never dropped until a walk
(`a_noted_path_not_reached_is_tried_again`).

## Reconciling Walk

Watches can miss a change: the service wasn't running, a network file
system reports nothing, or the machine's limit on watched folders
(`fs.inotify.max_user_watches`) is reached. So the writer also walks the
folder, comparing each file's size and modification time with the
index's: at its start, then every 10 minutes (`walk-every-secs` in
`src/limits.toml`). A lost event (inotify's queue overflowing) asks a
walk at once.

### Where No Watch Reaches

Where no watch reaches the whole folder (its limit reached, a folder
inotify refuses to watch, conversations a store keeps, or a system
memory has no watch for, such as macOS), memory walks every minute
(`walk-unwatched-secs`), and its status says why
(`a_watch_that_fails_says_why`).

## What a Call Waits For

A call waits for nothing. When the journal holds paths not yet indexed,
or the first walk hasn't finished, the call's answer says so, as it
says how far embedding has got, so the agent knows a file written that
moment may not be found yet.
