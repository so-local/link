# Fetch

> HTTP Requests With Shared Policy

`link-fetch` gives callers an HTTP client with bounded responses, explicit
transport policy, typed answers, and consistent errors. A caller starts with
the shared defaults and declares any required exception where it builds its client.

## The rules every call gets

`Client` is the client calls go out on, built by a `Builder` that says
what it allows, and it carries those rules to each call (`Request`). The
following defaults apply unless the caller explicitly selects an exception.

### Its user agent and timeouts

Link's user agent, `lnk/<version>` (`USER_AGENT`), and timeouts:
`TIMEOUT`, 30 seconds for a whole call, connecting to the last byte of
its answer, unless the caller says another, and `CONNECT_TIMEOUT`, 10
seconds to connect.

### No redirects

A redirect is an error (`Error::Redirected`) by default.
`Builder::follow_redirects(hops)` permits bounded HTTPS redirects for public
downloads. Requests on that client must carry no credential, since following
a redirect could otherwise carry authority to another endpoint.

### `https://` only

The default also permits `http://` to this machine (`is_this_machine`) for
local development. Elsewhere plain HTTP is refused (`Error::Insecure`) unless
the caller selects the metadata-server policy or explicitly permits it. A plain `http://` call
goes through no proxy. `localhost` and names under it resolve directly
to this machine's loopback addresses (`Loopback`), so network DNS cannot
redirect them. The explicit `plain_http` exception resolves other names
through normal DNS; metadata-server names use the fixed metadata address.

### An answer's size capped

`MAX_ANSWER`, 1 MiB, or the call's own `Request::limit`, read so: a
longer one is an error, never read whole first. `Streamed` reads an
answer as a stream instead.

### Typed both ways

A request's body is a `Serialize` type, an answer a `Deserialize` type
(`Request::answer`). By default, an answer is closed: a field its type
doesn't name is refused as `deny_unknown_fields` would, whether or not
the type says so. An answer that doesn't parse is an error naming the
route (`Error::Unexpected`), never a guess. `Builder::outside` permits extra
fields from an external API, and `answer_part` explicitly selects a partial
view while still checking the fields it reads.

### Errors map once

401 to signed out (`Error::SignedOut`), 429 to try again later
(`Error::Later`, with its `Retry-After`), a 5xx to the service's fault
(`Error::Failed`), any other refusal with what it said
(`Error::Refused`). None prints a credential, a query string or a
value the answer held: a route is named without a secret (`Route`).

### Credentials as `Secret`s

Credentials go in as `Secret`s (`Request::bearer`, `Request::basic`),
into headers marked sensitive; nothing here prints one.

## Where a call may go

`HttpsUrl` and `SendUrl` are the URLs a setting names, checked as it's
read: this machine's addresses and public ones, and how a host is
found (`Loopback`).

### Unix Sockets

For a local service with a Unix socket, `Builder::unix_socket` selects that
transport while retaining the URL, typed protocol and request limits. It
uses no network proxy and keeps the same URL policy.

## `blocking`

The same client, for a program that doesn't run on a runtime of its
own (a command, a sync service): every call made as `Client` makes it,
on a runtime this client keeps, and waited for. The same rules hold,
the same errors come back. Like reqwest's own, it can't be called from
a task on another runtime.

## Why it works this way

Why Link's calls share one client, and read answers closed, is on
[Fetch Decisions](fetch/decisions.md).
