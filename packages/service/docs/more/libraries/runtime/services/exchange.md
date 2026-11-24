# Program Exchange

One of Link's programs asks another for something by starting it, writing one JSON request to its standard input and reading one JSON reply from its standard output. `link_service::protocol` carries that exchange with bounded messages and a deadline, and stops the child with everything it started when the exchange returns.

## Exchanges

`exchange` takes the command to start, the request and a deadline. It writes the request and its newline, reads the whole reply and decodes it once the child has exited successfully. A failed, overproducing or blocked child is an error within the deadline. The caller supplies the command, so a test owns the program it starts and that program's environment.

### Message framing

Each request and reply is at most `MAX_MESSAGE_BYTES`, 1 MiB, and an oversized one is refused. `read` and `write` frame one message on a program's own standard input and output. `read_line` reads exactly one JSON line and leaves the bytes after it untouched, so a raw stream can follow a header on the same pipe.

### Process ownership

An exchange holds its child as an [`OwnedChild`](../../../../../../secretive/docs/more/libraries/files/secretive.md#ownedchild) in a process group of its own. The leader stays unreaped until the whole group is stopped, on success, error, timeout or unwinding, so a decoder panic still stops the child's descendants. Exit observation never releases that identity, and lost ownership refuses a numeric group signal. Descendants must remain in the owned group to be stopped with it.

## Raw byte progress

`copy_flushed` copies raw bytes with an 8 KiB buffer, using complete writes and flushing every batch before its next read. It flushes at end of input, returns the copied byte count and propagates read, write and flush errors. Interrupted reads retry. Binary frames need no newline to become visible, and copying uses explicit reads and writes rather than a specialized socket-to-pipe splice. The caller owns deadlines and cancellation.

## Acceptance

The tests run real children in owned process groups. Successful exchanges return the decoded reply, and failed, overproducing and blocked children fail within the deadline. Oversized headers are refused while following stream bytes stay untouched. Cleanup is checked after leader exit, after a decoder panic and when the child's output stays held open.
