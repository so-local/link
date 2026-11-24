# Runtime

`link-runtime` lets synchronous entry points run async operations on one
Tokio runtime kept for the lifetime of the process.

## Running async work

`block_on` accepts a future returning `Result` and returns its value or
error. The first call creates a multi-threaded runtime with timers and
I/O enabled; later calls reuse it, including calls from other threads.
Tasks spawned by one call can continue running between calls.

## Calling context

Call it from synchronous code. Code already on an async runtime awaits
its future directly.
