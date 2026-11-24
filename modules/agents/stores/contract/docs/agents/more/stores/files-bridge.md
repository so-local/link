# Files Bridge

How an agent's Home kept in a bucket is reached from its sandbox, and
the storage credential the bridge holds, both in `link-store-contract`.
[Every bridge](../../../../../../../security/sandbox/bridge/docs/security/sandbox/more/bridges.md) speaks `link-sandbox-bridge`'s calls.

## Reviewed Writes

Serving owners can enable `Homes::reviews_writes` and implement `Homes::review_write` with a shared interaction review. The default preserves the existing write policy. The source freezes owner, agent, team, path, bytes and expected object version before asking; it never holds an authority or storage lock across the owner wait. Read grants and independent resource policy continue to apply.

### Commit the Reviewed Version

After review, the source takes the dispatch fence, reloads its binding and team grant, and claims only the exact prepared payload. The object write uses the frozen version or absence condition. A concurrent target change fails as a conflict, and revoked access prevents application. An unavailable source outcome stays Unknown. A backend without conditional writes cannot enable this review path around an unconditional write.

## A storage credential

A bridge that reads and writes a cloud's storage for the agent is
handed a credential, never the cloud's own sign-in
(`link_store_contract::authority::StoreSecret`): `{"token", "expires",
"refresh_url", "refresh_token", "headroom_bytes"?}`, an access token
sent as Bearer, when it ends (Unix seconds), where and with what to get
the next one, and, optionally, how many bytes it may still add.

### Its refresh

`link_store_contract::authority::Held` refreshes the token with OAuth's own
refresh (RFC 6749, section 6: `POST refresh_url`, form-encoded,
`grant_type=refresh_token&refresh_token=<grant>`, answered
`{"access_token", "expires_in", "headroom_bytes"?}`) once two thirds of
its life have passed.

### Where it refreshes

Over HTTPS, or HTTP only to this machine's loopback (`127.0.0.1`,
`::1`, `localhost`, by the address's parsed host), never following a
redirect.

### Its room

It takes each answer's `headroom_bytes` as the room left. A write that
would pass it is refused before anything is sent (`Held::reserve`). A
refresh answered 400 or 401 ends the credential: every call after it
fails.

## The files bridge

The files bridge (`link_store_contract::files`) serves an agent's Home
kept as objects in a bucket.

### Its halves

Its client is the `files-client` feature of this crate (Link file's
`bridge` command, Link Memory's `serve-many`); its server half (`serve`,
`Homes`, `DirStore`, and `Crypt`, rclone's crypt format) is the
`files-bridge` feature, for whatever serves it, so neither tool builds
it. `files` is both.

### Who serves it

No part of Link serves an agent's Home through the files bridge or runs
`lnk-file-tool bridge`, nor binds an agent's Home to `lnk-memory-tool
serve-many` through it: a machine that runs agents only while they
answer, with Home in a bucket, does. An agent's own bridge serves one
for [the agent's teams](#a-teams-folder-elsewhere) whose folders their services answer.

## A team's folder

A `read`, `write` or `list` may name a `team`, `<name>` (a team of the
agent's own owner's, by the name its sandbox file's
`[teams.<team>]` gives it) or `<account>/<name>` (another
account's). A name never holds a `/` and an owner is an account's, so
the two forms never meet.

### Reached at each call

It then reaches that team's folder instead of Home, as the server's
`Homes::team` gives it at that very call, never what was true when the
agent was bound, so an agent taken off a team, or a team renamed or
removed, is refused at its next call.

### What it reads and writes

It reads the folders of it its grant writes or reads, writes only those
it writes (`.` for all of it), each from the version read, and lists
only what it reaches. Each write is told to the server
(`Homes::team_changed`), which keeps the team's memory current.

### Its index

An `index-get`, `index-put`, `index-list` or `index-delete` naming a
team reaches the index kept beside that team's folder, in the folder's
storage, the team's memory's. Since the index tells what's anywhere in
the folder, it's read only by a member granted all of it (`.`, read or
written), and written or deleted only by one writing all of it that the
server lets keep it (`TeamFolder::keeps_index`, as whoever pays for it
says), only as the index's own objects (`<engine>/manifest.json` and
its segments), within `TEAM_INDEX_MAX`. `InTeam` is the caller's side, a store named as
Home's is.

### Store capacity

`Store::put_bounded` and `delete_bounded` ask for atomic shared-prefix
capacity. A store without that authority refuses before mutation.
Team index mutation uses these operations, without an unlocked listing.
`DirStore` keeps prefix counts under its cross-process folder lock,
syncing a dirty record before mutation and the completed count after.
Recovery recounts interrupted writes; ordinary writes to a registered
prefix preserve the same bound. Separate handles share its count.

### Membership authority

Every call naming a team passes one check of the member's grant
(`team_allows`) before anything of the folder is reached. A call's
unknown field is refused, never ignored.

## A team's folder elsewhere

A team whose folder the bridge neither holds nor checks, such as a
hosted team's folder in its owner's bucket reached by a member on its
own machine, is answered by that folder's service. `Homes::remote_team`
names the service at each call naming the team, before `Homes::team`,
and `RemoteTeam` hands the call on. The folder's secret and storage
credential stay with the service, never on the member's machine.

### The route

Each call is a `POST` to `team_route(owner, team)` under the service's
URL, `/v1/teams/<owner account>/<team ID>/files`, whose body is the
call's `Action` as JSON with its `team` left out. A success answers the
`Answer` the bridge would. A refusal answers its status with the
bridge's `Failure` envelope, 409 for a conflict, which the member's
client meets as the bridge's own conflict.

### The member's token

The call carries the member's own access token as `Authorization:
Bearer`, made outside the sandbox by its machine for the service's host
with `member_grants(team)`, `team.member@team:<id>`
(`RemoteTeam::for_agent`, the way the secret bridge makes an agent's
token). The bridge keeps it until a minute before it ends, and it
never enters the sandbox.

### The service's checks

The service takes no identity from the call's body. It derives the
member from the verified token, reads the team's roster at that call,
and answers through `TeamCalls`, the bridge's own checks
(`team_allows`, the member's `team_role`) for the folder as that member
reaches it now. A member taken off the team is refused at its next
call. A write its source reviews is refused there, since a review needs
an agent bound to a bridge.

### On the member's own machine

`RemoteTeams` is the `Homes` of a bridge serving one agent's teams kept
elsewhere, by the names the agent gives them, and no Home. It takes only
that agent's calls, and reaches a team only while the team's file there
still lists the agent, asked at each call. `RemoteTeam::with_grant`
holds each call first to what the agent is granted there, its sandbox
file's folders with its roles', by the bridge's own `team_allows`. The
service then checks the call again as the member its token names.
