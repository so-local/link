# REST Transport

Every action declares what it does to state, its effect, and how its
answer arrives, and HTTP is derived from those declarations: the method,
where the inputs go, the status of each error, whether an answer may be
cached, and how a stream is framed. Nothing about HTTP is written in an
action's declaration. `transport.toml`, beside `link-actions`, holds what
the derivation reads, and `link_actions::transport` reads it once.

## Methods

An action's [effect](actions.md#effects) gives its HTTP method. Its path
is its ID's words under `/_link`, as `/_link/env/approval/add`, so an
action is named by what never changes.

- **`find`:** reads, changing nothing anywhere, as `GET`.
- **`insert`:** makes a new thing, as `POST`.
- **`update`:** changes a thing that exists, or sets it, as `PUT`.
- **`delete`:** removes a thing, as `DELETE`.

## Inputs

Where an action's inputs go follows from its effect. A `find` and a
`delete` take every runtime argument as a query parameter and no body,
so a `find` is cacheable and a `delete` carries nothing a proxy may drop.
An `insert` and an `update` take the argument naming what they act on,
the action's `resource`, as a query parameter. Every other argument, and
the specification beside them as `spec`, travel in a typed JSON body, so nothing
a person wrote travels in a URL, where proxies and logs keep it.

### Arguments REST Leaves Out

REST accepts an action's arguments except those saying where or how a
machine runs it, core's shared `workplace` group, `--cloud`, and their
siblings, listed once in `format.toml`'s `[rest] never`, since a REST
call is answered where its service runs, never on a machine the caller
picks. An action may accept fewer, naming them in its
`rest-args`. An argument REST doesn't accept throws `bad-request`.

## Formats

A request names its body's format in `Content-Type` and its answer's in
`Accept`, and an answer without `Accept` is JSON. Each shape is declared
once in `types.toml`, independent of format. JSON (`application/json`) is
the format offered, and `transport.toml` names TOML
(`application/toml`) and XML (`application/xml`) for when they're
offered too.

## Caching

A `find` declares how long its answer keeps, and whether it's every
caller's, in its own words, never as a header:

```toml
# actions.toml
schema = 1

[actions."model.model.list"]
description = "List the models Link knows"
keeps = { for = "1h", shared = true }
```

`for` is a duration, or `forever` for an answer that never changes, as a
file fetched by its SHA-256. An answer is one caller's own unless it says
`shared = true`, and one caller's own answer is never kept by anything
between. The transport writes `Cache-Control` from it: `private` or
`public` with its `max-age`, `immutable` for one that keeps forever, and
`no-store` for a `find` that declares nothing. `keeps` is refused on any
other effect.

## Errors

An error is one typed value, the same shape for every action, thrown
where the problem is found, complete, and never assembled after it's
caught. Core declares its shape, `core.error`, so every surface has it.

### Named Errors

Each error is thrown by its name, and its name carries its HTTP status,
from `transport.toml`, so code says what went wrong and never writes a
status. A test holds the names core declares and the statuses here to
the same list.

- **`400`:** `bad-request`.
- **`401`:** `unauthorized`.
- **`402`:** `payment-required`.
- **`403`:** `forbidden`.
- **`404`:** `not-found`.
- **`409`:** `conflict`.
- **`413`:** `too-large`.
- **`429`:** `too-many-requests`.
- **`500`:** `internal`.
- **`503`:** `unavailable`, with `retry-after` saying when to try again.

A file saved over a newer version throws `conflict` where the revisions
are compared. A thing that isn't the caller's throws `not-found`, so its
existence doesn't leak.

### Public and Private

Every error has two parts, the same for every error. Its public part,
`core.error`, is what the caller may see, sent across the wire and shown
to a person: its name, a message written for them, and where the action
names one, a `reason`, a word the caller can act on. Its private part,
`core.error-private`, is what only the server keeps, written to its logs
and never sent: the cause and the values involved.

### In Rust

In Rust, `link_types::Thrown` holds both. The code that finds a problem
throws it with both parts filled, code between passes it on unchanged,
and where it's caught, its name gives its status, its public part
becomes the answer's body, and its private part goes to the logs.

```rust
use link_types::{ErrorName, Thrown};

let thrown = Thrown::new(ErrorName::Conflict, "The file changed since you opened it")
    .reason("stale-revision")
    .value("path", "instructions/main.md");
assert_eq!(thrown.status(), 409);
```

## Streams

An action whose answer arrives as it's made declares `stream = true`,
and its output is the union of the events it sends, a `oneOf` of records
told apart by a tag, which the catalog's check holds:

```toml
# actions.toml
schema = 1

[actions."model.model.chat"]
description = "Ask the model with one typed request, its answer streamed"
output = "stream"
stream = true
```

A stream is Server-Sent Events (`text/event-stream`), one framing for
every format. Each event's name is its type's tag, so a reader knows its
kind without guessing, and its data is the event. Its last event says how
it ended, and a failure's public part is an `error` event. A stream that
breaks before its ending is seen as unfinished.

## Generated Clients

[The OpenAPI document, the TypeScript client, and the Rust client](../../../../../surfaces/docs/more/developing/modules/catalog/surfaces.md) each
derive the method, query, body, errors, caching, and streams from these
declarations.
A streaming action's client method yields its typed events, one at a
time.
