# API Client

`link-api` is Link's typed client for the account service's API, one
function a route, on `link-fetch`. It holds no credential of its own,
and it makes and checks the access tokens the service's routes take.

## Its types

`types` is generated at every build from `schemas/account.yaml` and
`schemas/remote.yaml` by `build.rs`, using typify. Objects are closed and
string patterns are checked as an answer is read, with credentials held as
`Secret`. Callers still enforce semantic and numeric bounds not represented
by the generated Rust type. A contract change starts in the owning schema.

### One function a route

Each is named as the specification's `operationId`, takes the route's request
type and answers its answer type. `ROUTES` lists each method, path
under `base_path()` (`/api/v1`) and function, and a test holds them to
the specification's paths.

### Its protocol version

The API's version is a protocol's, which `src/protocol.toml` declares
(`link_api::PROTOCOL`, now 1): every call states it in its path,
`/api/v<current>`, and a test holds the specification's server path and
`info.version` to it. The service serves each version [from `oldest` to
`current`](../../../../../version/docs/more/libraries/types/version.md#protocol-versions) under its own path, so a change the API can't make by adding
to a version advances it, with the older path kept until `oldest` passes
it.

### The report's answer

The one answer read entry by entry, not closed, is the report's
(`Asked`): an action of a kind this installation doesn't know is refused
alone, so a newer service never stops an older installation's actions.

## Its calls

`Api` is the service at one URL, cheap to clone. A call that needs a
credential is handed it, a `&Secret`, by its caller. `Api::sign_for` signs
an assertion with the installation signer supplied by that caller and requests
an access token. The client never chooses a local credential store.

### `link-fetch`'s rules

`https://` only (or `http://` on this installation), no redirects, an
answer's size capped, and errors mapped once: a 401 is
`Error::SignedOut`, a 429 `Error::Later`. What a refusal says, in the
specification's words, is `refused`.

### Installation Sign-In

Call `Api::start_device` with the typed enrollment request, then use
`Api::poll_device` while the person completes approval. Read a refusal's
`ErrorCode` through `refused`; AuthorizationPending is distinct from a
permanent refusal. The caller owns polling, credential persistence, and
what it presents to the person signing in.

### Tokens and Signers

`Api::sign_for` signs an installation assertion and exchanges it through
`Api::token` for the requested scope and audience. `sign_for_agent` binds
the service principal and supplied grants instead. `Api::keys` fetches
public verification keys, while the local JWT helpers check tokens without
a network request. Keep token values in `Secret`, outside ordinary output.

### Installation Records

`Api::list_installations`, `grant_installation`, and `sign_out_installation` operate on
typed installation records. `logout` signs out the token's own installation. These
methods issue the declared request with the caller's credential, and the
remote endpoint validates the account and resource authority. The schema
owns request fields and response shapes.

### Agent Reports

`Api::report_agents` submits an installation's typed report and returns `Asked`.
An empty response yields no requested actions. An action the current client
does not understand is refused individually while recognized actions remain
available. The caller still owns executing or refusing each returned action.

## Access tokens (`jwt`)

Access tokens and the assertions installations sign for them, as the specification
has them (`InstallationAssertionHeader`, `InstallationAssertionClaims`,
`AccessTokenHeader`, `AccessTokenClaims`): compact JWS, Ed25519 alone.

### `assertion`

An installation signs an assertion with its key for each access token it
asks for: one request, at most `ASSERTION_LIFE`, 60 seconds.

### `check`

Whatever an access token is for checks it with `check` and the account
service's public keys alone (`Keys`), never asking the service. The
header is read first: `alg` `EdDSA` and `typ` `at+jwt` alone, its `kid`
one it holds. Then the signature, then the claims.

### The claims

Closed: the issuer it trusts, its own audience, a subject of a kind it
takes, a grant of what it's asked to do (`Grants::allows`), and a life
no longer than its own longest, with no allowance for clocks.

### Account stops

`stopped::Cache` merges snapshots outside requests, retaining one maximum cut
per account without duplicate entries. Observed stops stay 25 hours; a caller
may include historical cuts still relevant to its current authority. Older or
empty responses cannot restore authority within that window. Checkers fetch
every 30 seconds and retain the cache on failure. The service chooses its
startup policy and persists revocations of longer-lived credentials, such as
API keys.

### Refreshed authority

A refreshed token may retain its original authorization in
`auth_time`, which cannot exceed `iat`; stop checks and `jwt::authorized_at`
use it instead of the token's later mint time, so refreshing cannot erase a cut.
