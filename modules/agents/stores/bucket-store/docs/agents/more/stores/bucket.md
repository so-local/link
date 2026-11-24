# Bucket Store

`lnk-bucket-store` is the `bucket` place's driver: an agent's
collections as objects in its bucket, the first encrypted one
`lnk storage` connected for it, through `lnk storage`'s engine. It keeps
[every collection](../../../../../contract/docs/agents/more/stores/protocol.md), jobs too, and the storage bridge runs it outside every
sandbox. Inspect the connected clouds before selecting bucket storage.

```sh
lnk storage list
```

## Its objects

Every collection is objects under `.link/agent/`, which a push, a pull
and a clean of its Home never touch, as `link_store_contract::store::objects`
keeps them over any bucket of versioned objects: each event, lease,
list of tools, job and file an object, each change written only at the
version read.

### Versions

An object's version is the SHA-256 of what it holds, and a write that
names one reads the object first, under a lock on this machine for the
agent's bucket.

### Logical File Revisions

The [object-store implementation](../../../../../contract/src/store/objects.rs)
wraps each logical file in a strict record with a nonzero revision and
its base64 contents. Deletion retains a revision-bearing tombstone, hidden
from file reads and listings, so recreation advances the existing revision.
The caller’s numeric file revision is distinct from the backend’s content
hash used for conditional object writes.

### Its names

A conversation's name is read back from one spelling only (its escapes
in uppercase hex), so two objects can't pass for one conversation.

## On one machine at a time

**It's safe on one machine at a time only:** two machines writing one
agent's bucket at once aren't kept apart, as an agent runs on one
machine at a time and a move pauses it where it was before it answers
where it goes. An agent copied by hand and started on two machines over
one bucket isn't kept apart.

## Its encryption

Its encryption (rclone's crypt) keeps names and contents unreadable but
doesn't bind an object to its name or its time, so whoever can write
the bucket can put back an object it held before, an older lease or
list of tools among them, though never write one of its own that reads.

### A bucket it refuses

A bucket that isn't encrypted is refused, and so is a call for an agent
with no bucket, or a name that isn't an agent's ID. The bridge takes
each call only as the agent's own.

## Its tests

The [shared driver checks](../../../../../contract/docs/agents/more/stores/protocol.md#test-a-driver) define the event, lease, file, and job obligations for a bucket driver. Exercising this adapter also needs a connected encrypted bucket and its storage engine; shared contract fixtures alone do not establish that provider’s behavior.
