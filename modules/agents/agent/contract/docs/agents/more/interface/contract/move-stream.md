# Move Stream

The stream an agent moves in, as every end of it reads and writes it:
`lnk agent state-send` and `receive`, a cloud that hosts agents (`agent-receive`,
`agent-send`), and `lnk storage` for such an agent's Home
(`link_agent_contract::moving`).

## The stream

A tar whose first entry is `lnk-move.json`, the manifest, and whose
last is the empty `lnk-move.end`: a stream without it was cut short,
and is refused.

### Its protocol version

The manifest's `format` is the version of the move protocol its sender
speaks, which `moving/protocol.toml` declares (`moving::PROTOCOL`, now
4). A stream is one-way, so its sender hears no version back: the
receiver reads one from `oldest` to `current` and refuses any other by
name, an older one naming the sender's upgrade and a newer one its own, in `check_format`, by [protocol version](../../../../../../../../../packages/version/docs/more/libraries/types/version.md#reading-a-one-way-message).

### Each file's hash

Every file comes after a PAX header `LNK.sha256`, its SHA-256 in hex: a
receiver hashes what it writes and refuses the stream when one differs
or has none.

### What it may hold

Only files, folders and links, each a plain path under a folder its
phase allows (`files/`, `memory/`, `harness/`, `connections/`). `files/`
is the agent's folder as the agent sees it, and `memory/<engine>/` its
memory engine's index.

## The manifest

`{"format": 4, "phase", "move", "removed"?, "version", "agent": {"id",
"name", "owner"?}, "harness", ...}`; the rest of it is [the sender's to
say](../../../../../../docs/agents/more/interface/README.md#lnk-agent-state-send).

### `phase`

`copy`, `switch` or `files`: a copy while the agent may run, the switch
that puts it in place, or Home alone, for an agent a cloud account
hosts.

### `move`

The move's ID, 128 random bits in hex, the same in its copy and its
switch, so a receiver refuses a switch of another move than its copy.

### `removed`

In a switch, the paths of `files/` and `memory/` the other end holds
that are gone, which the receiver takes out.

## `have`

A receiver's `have` is `{"<path>": "<sha256>"}`, each file it holds by
its path in the stream (`files/...`, `memory/...`). A sender skips each
it holds the same, so a copy tried again, or a move back to a machine
that kept the agent's files, sends only what's new.

## Collections

Collections cross through their source and receiver's store contracts
(`link_agent_contract::moving::collections`), including `file`.

### What a receiver replaces

A receiver replaces instructions, skills, notes and scheduled jobs with
what the stream holds, and appends the conversations' missing events
after checking their shared prefix. Conversation tool narrowing travels
with each thread, including one without events. Notes travel at
`memory/notes.md`.

### Schedules

A switch also carries `schedule/<place>/<id>.json`, each the store
contract's `Job`, where `place` is `waiting`, `firing`, `ending` or `ended`, and
`schedule/day.json`, the pair `[day, sent]` from `job_day`. Schedules
are sent only while paused, in full on each switch.

### One form

Each event (a line of a conversation's `events.jsonl`), each job and
`day.json` is compact JSON with every object's keys sorted, at every
depth, whatever features the sender was built with: two builds hash the
same conversation the same, so a move never resends it for its form.

## The sealed envelope

A switch's complete tar payload is a `link_secretive::stream`
envelope, sealed to the key `lnk agent receive` printed, with HPKE
context `{"owner":"move","kind":"stream","version":1}`.

### Its header

It starts with `LNKSEAL1`, a big-endian 32-bit HPKE-header length (at
most 4096), then that sealed stream key. A stream gets a fresh random
32-byte key.

### Its records

Each record has a big-endian 32-bit plaintext length (at most 65536),
followed by ciphertext and its 16-byte tag. The nonce is four zero
bytes plus a big-endian 64-bit record counter, and the authenticated
data is that counter and the plaintext length.

### Its end

A zero-length authenticated final record is required; bytes after it
are refused. Counter overflow and a failed record write make it
unusable.
