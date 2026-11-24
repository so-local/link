# Harness Base

`link-harness-base` holds what every harness shares apart from how Link
runs it: the proof an endpoint gives Link's challenge, the folders an
agent sees and may write, the programs it may run, and a harness's
linked state.
Its code is in [`link-harness-base`](../../../../../README.md).

## The endpoint's proof

[An endpoint proves it holds the agent's key](../../../../../../docs/agents/harnesses/more/protocol/endpoint.md#proving-its-the-harness) before Link's connections
send it the key or a message.
The challenge comes on `GET` at `PROOF_PATH`, `/_link/proof`, in the
`Link-Challenge` header, and the answer goes back in `Link-Proof`.

### A challenge

`is_challenge` takes a challenge of 32 to 128 hex characters, and an
endpoint answers no other.

### The proof

`endpoint_proof` is the answer only the key's holder can give:
HMAC-SHA256 of the challenge with the key, in hex, after a label of its
own, so a proof is never a signature of anything else made with the
key. It's never the key itself, and of no use for any other challenge.

### Checking one

`proof_matches` says whether a proof is `endpoint_proof` of a challenge
with a key, compared in constant time.

## The programs it may run

`Commands` is a sandbox file's `[commands]`: the programs an agent's
`run` tool may start, each a name found on the system's `PATH`, an
absolute path, or `*` alone for any. An empty list is none, and no
`run`.

### Checking the list

`Commands::check` refuses an entry that isn't a program's name (1 to 64
letters, digits, `-`, `_`, `.` and `+`, not starting with `.`), an
absolute path without `..`, or `*` alone. An absolute path is at most 1,024 bytes, ends in a filename rather than `/`, and contains no control character.

### What this machine has

`Commands::present` gives the entries this machine has, each as it was
written, by `find_program`: a name looked up in a `PATH`'s folders, an
absolute path as it is, each a file that runs, then each of its
toolchains' programs this machine has, by name. `allow = ["*"]`, alone,
is any program (`Commands::any`), and `present` gives it as written,
until a sandbox made for toolchains replaces it; `*` beside other
entries is refused.

## The folders an agent sees

`layout` names the folders of an agent's folder, lowercase and side by
side, never nested, since a sandbox has no read-only folder inside a
writable one: `instructions`, `skills` and `chats` in its own
folder, and Home's `work` and `memory`, made at each start.

### Its files

`STANDING` is its standing instructions, `instructions/instruction.md`,
and `NOTES` the notes its memory's `remember` writes,
`memory/notes.md`.

### What it may write

`Folders` is a sandbox file's `[files]`: the folders an agent may write
and those it may only read, by name, and of each of its teams' shared
folders, what it may write and only read. A team not named there, it
can't reach at all.

### Link's grants

Left out, Link's grant is its work written, its conversations read
only, and the rest, its instructions and skills, read.

### Checking the grants

`Folders::check` refuses a name that isn't a folder's (lowercase, one
level), one named twice or in both lists, `chats` among those
written, or `.` beside a folder of Home's, which `.` holds.

### Asking what it may write

`writes` says whether the agent may write a folder, one named or, under
`.`, any of Home's; `writes_notes` whether memory may write its notes,
where it may write its instructions or they aren't kept from it; and
`whole` whether it writes all of Home.

### A team's folder

`TeamFolders` is what of a team's shared folder an agent may reach:
folders of it it writes, and those it only reads, each by its name in
the folder, or `WHOLE`, `.`, for all of it. The rest is closed.

### Checking a team's grants

`TeamFolders::check` refuses a name that isn't one of the shared
folder's folders, one named twice or in both lists, and `.` beside a
folder it would hold that's reached otherwise, since the sandbox can't
keep a folder only read inside one written.

## A harness's state

`State` is what [`lnk agent move`](../../../../../../../../../agent/docs/agents/move/README.md#move-your-agent-to-the-cloud) carries of a harness's home to another
machine: its state, never its install, which is made fresh there, since
a Mac's programs don't run on Linux. `include` holds the folders and
files that hold the agent, and `exclude` the paths under them that
don't: its programs, runtimes, logs, caches, and the keys `configure`
writes again. Each path starts with `~/`, its home.

### A link not made

`NOT_LINKED`, 10, is what `link` exits with when a connection needs
linking and there's no terminal to link it on. It is the connections'
own code, so no other failure, such as a wrong flag, exits with it.

### An endpoint's port

`endpoint_port` reads an adapter's `endpoint`: `http` on this machine's
loopback, `127.0.0.1` or `localhost`, with an unprivileged port from
1,024 to 65,535. Its optional path contains only printable ASCII bytes.
Other schemes, hosts, ports, or paths are refused.
