# Developing

`lnk-code-tool` is Link Code: an MCP server over stdio that asks a
project's language servers for diagnostics, definitions and
references, each server in a sandbox. How to use it: [Link
Code](../README.md).

## Build and test

From the repository root, run `cargo build -p link-code-tool`, then `cargo test -p link-code-tool`.

## How it's put together

`serve --files <folder> --programs-path <PATH> --timeout <duration> --servers
<json> [--unavailable <language>=<why>]... [--var <NAME>=<value>]... --
<sandbox>...` is the tool. With no sandbox after `--` it refuses to
start.

### What Link gives it

Link gives it everything at a start: the specification's `[tool.code.servers]`
as JSON, each language whose server the sandbox file's `[commands]`
doesn't allow or this machine doesn't have with why, the call's timeout
a tenth less (2 s at least), and the sandbox, [`lnk sandbox run`](../../../../../../../../security/sandbox/docs/security/sandbox/guide.md#sandbox-guide) with the flags the
sandbox file makes, no network, `[commands]`' programs and, on a Mac,
the developer tools when `[commands.macos] developer_tools` grants them.

### Variables in the sandbox

Each `--var` is set inside the sandbox by the program the contract's
`src/programs.toml` names, `/usr/bin/env`, which Link adds to the
sandbox's allowed commands when there are variables to set.

### `src/lsp.rs`

The client: one server, `<sandbox> -- <server>` in its project's root,
in a process group of its own, killed when the tool stops.

### When the tool dies

The group is also killed if the tool dies, however it dies, on Linux
and on a Mac alike. A server never looks for its parent, so a small watcher process,
forked in the server's group before the server runs, in [`watched_by_parent`](../../../../../../../../../packages/secretive/docs/more/libraries/files/secretive.md#watched_by_parent),
waits on a pipe only the tool holds open, and when it closes, kills the
group. The watch follows the tool's process, not the thread that
started the server, so a server outlives the call that started it. Its
`initialize` names no `processId`: a server in the sandbox's own process
namespace can't see the tool's, and one that polls for it would end.

### Its messages

A thread reads its messages, framed by `Content-Length`: replies,
published diagnostics, `$/progress` and `experimental/serverStatus`,
and the few requests a server makes of a client. It answers those as a
client offering only the server's `settings`: each section a
`workspace/configuration` asks for, after one
`workspace/didChangeConfiguration` at its start when it has any.

### No edits, no watching

It applies no edits and refuses a server's registration for
`workspace/didChangeWatchedFiles`. Instead, each call looks over the
server's root for the files it serves and its root files, through no
link and past the folders `unwatched` names in `src/limits.toml`, at
most `watched-max` of them, and tells the server which were created,
changed, or deleted since the last call. So a server that watches no
files of its own, as gopls, still sees a change to a file it indexed
but wasn't given.

### Bounds

A server is unconfined text to this program, so `src/limits.toml` bounds
each part of what it sends: a header line, a message, a file's
diagnostics, and the work in progress followed. A reply is kept only
for a request a call still waits on. A diagnostic's URI is named as
this client names its path, however the server encoded it. The same
file bounds an answer, holds the client's timings, and lists the
protocol versions it speaks to a harness, newest first.

## How a call is answered

What each call reads, gives its server again, and asks it.

### A file and its diagnostics

A file is read through its handle each time a call needs its text, opened folder by
folder from the files folder's with `O_NOFOLLOW`, as [`nofollow`](../../../../../../../../../packages/files/docs/more/libraries/files/files.md#folders-another-may-write-nofollow) says,
and that text is what the server is given and positions are counted in.

### Given again each call

A file is opened on the first call about it and changed and saved when
it changed on disk since. Each call gives a server every file it was
given before again, closing one that's gone and dropping its
diagnostics. A file's version is taken and sent under one lock, so two
calls give a server its versions in order. Diagnostics wait until the
server has no work in progress and has been quiet half a second, and,
for a file given new text, has published past what it had or stayed
quiet three seconds; a file given unchanged has nothing new to wait
for.

### A symbol

A symbol is found with `workspace/symbol` (`src/symbols.rs`), exact names only, and its
name's own position in its declaration, for `references`. Every exact
match is kept: `definition` lists them, and `references` refuses more
than one.

### Servers Starting

Each server has a slot, found under one lock held only to find it. A
call starts its server holding that slot's lock alone, so a server
starting holds up only the calls to it.

### `src/lib.rs`

It refuses a file outside the files folder (links resolved), picks the
server by extension, and turns positions both ways: the model's
characters, the server's UTF-16 units. A line is read only from a file
in the files folder.

## Tests

`tests/protocol.rs` (`harness = false`) runs itself again as a fake
server (`--fake-lsp`). The diagnostics-waiting case reports work and
publishes past the client's half-second quiet period. Other requests
publish immediately.

### Fake Server Cases

The fake crashes on `CRASH`, begins work it never ends on `BUSY`, holds
its start in a root named `slow` until the test releases it, and names a
file outside the files folder.

### Real Server Case

`tests/servers.rs` runs `rust-analyzer` on a
small crate with an error under `lnk-sandbox`, with Link's own server
settings, so a function is found by name beside a type of a close name
and two `new` are both named.

## Provider Calls

The standalone Code server publishes `diagnostics`, `definition`, and `references`. Managed agents receive those operations through the `file` namespace rather than a separate Code server name.

### `code__diagnostics`

For an agent, use `file__diagnostics` as described in the [guide](../guide.md#diagnostics). The Code provider’s `diagnostics` implementation still owns language-server requests and their timeout.

### `code__definition` and `code__references`

For an agent, use `file__definition` and `file__references` in the [guide](../guide.md#definitions-and-references). A direct MCP client chooses its server name and calls the provider’s `definition` or `references` operation.
