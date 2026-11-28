# Secrets Contract

`link-secret-contract`, the library every program that keeps or gives a
secret reads it through: the store, its grants, the keeper's protocol
and the secret route's types. [`lnk-secret`](../../../../../docs/config/secrets/README.md) is its command line.

## The store

A user's secrets are kept as files, `secrets.toml` (mode 600) and its
lock beside it: yours in Link's settings (`~/.lnk/safe/secrets/secrets.toml`,
`path`), or one the keeper keeps for a user (`Store`). Each secret is
kept under its ID (`[entry.<id>]`), written `secret:<id>` (`SECRET_KIND`,
`resource`), minted under the store's lock and retired when it's
deleted (`retired`), never given again. Each value is in the Keychain
where there is one, its account `secret/<id>` (`KEYCHAIN`). A store of
the shape before IDs, each secret under its name, is refused.

### Reading it

`list` gives each secret kept for you, by ID (`Listed`): its kind
(`SecretKind`), named or own, its name, or for an own secret whose it is
(`Owner`), how it's given and where, never
its value, and those no longer kept but with copies left elsewhere, by
their copies alone. A command names a secret by its name or
`secret:<id>` (`Called`), never told apart by an ID's shape.
`for_service` gives what a service is granted, by its ID, its own or
what [`lnk secret add`](../../../../../docs/config/secrets/README.md#grant-access-to-a-secret)
gave it; a value the Keychain should hold and doesn't is an error,
naming it.

### Read as you

`for_you` gives your own named secrets of the IDs asked, read as their
holder, for what you run yourself: an environment module gives an
environment the secrets you approved for it this way, and the secret
module names no environment. One deleted since is left out.

### Writing it

`add` keeps a secret under a new ID, sealed when it's sealed, given
where its grant says, replacing whatever was kept by that name, whose
ID it retires; it gives the new ID. `grant` adds and takes grants of a
secret named by name or ID. `keep_own` keeps a service's own secrets,
dropping one no longer its own, and is written only when something
changed. `clear` removes every secret kept
for you, and their store, for [`lnk uninstall`](../../../../../../../../docs/more/modules/guide.md#uninstall).

### Own secrets

A service's own secret has no name: it is found by its service and its
`Use`, `Use::Own` with a purpose the module that keeps it names
(`model`, `connection:telegram:bot_token`), so a rename changes none of
it, and `grant` refuses it. The contract checks a purpose's shape and
never reads its meaning. `Use::flag` writes a use for a command line,
and `Use::from_flag` reads it back.

## Grants

`Grant` is a principal, an action and a resource, the secret it's kept
under. The principal is the service a workload acts as, `service:<id>`
(a `link_principal::Principal`), never the workload. The action follows
how the secret is kept, `Action::Pass` for a sealed one and
`Action::Read` for a plain one. Beside them are the workload,
`<kind>:<id>`, and the name it was granted by, to show, and `Use`, how
it's given. A grant to anything but a service, or of another action, is
refused as the store is read. `Grant::new` makes one for a `Grantee`,
and `service_principal` names a service.

### Who a grant names

`lnk secret add --to <kind>:<name>` asks the modules that keep
workloads where the one of that kind and name is (`GRANTEE`, the
question `secret.grantee`, answered with a `GranteeAt` in this
contract's types, at protocol 2).

#### Its answers

Its answers are Stable IDs': `here`,
with the workload's ID and the principal it acts as, `service:<id>`;
`unstarted`, with the command that gives it an ID; `on`, the machine
it's on; `none`, of a kind the module keeps but nothing of that name;
and `unkept`, a kind it doesn't keep.

#### The grantee

`grantee` gives the `Grantee`,
the service it acts as and its workload, or an error naming the machine
it's on, where its secrets are given it, or the command that gives it an
ID. The kind is
the answering module's word, and the contract names none.

## Copies a move gives

`copied` notes a place (`Place`, a box or an account by its name) for
each of a service's own secrets before its owner sends them there,
never a secret `lnk secret add` gave it. Once its entry is gone from
here, the copies stay under the secret's retired ID with the service's
ID and the use (`[left.<id>]`). `gone_from` takes the note off once the
service has left that place, and `forget` takes back every grant to a
service whose workload leaves or is removed.

### Secrets kept while away

`retain_own` parks selected own uses in the keeper while the service's
workload is away. `forget` removes its live grants but keeps those
values, and `keep_own` restores them for the same stable ID. `remove`
deletes retained values too. `copied_without` omits uses the place never
receives, so they are never listed as copied there.

#### Following a copy away

`gone_from_without` preserves those uses at the source after the
service leaves it. Copy records keep each own use, so an owner can
follow some uses away while leaving others at their source box.

### Removing a secret

`remove` takes a `Removal`: a secret by its name or ID, here and at
each copy, its ID retired; or, on a box a copy reached, a service's own
secret by its `Owner`, here alone. It notes every copy as left before
any is tried, and takes each reached off after, so nothing is forgotten
if it's cut short. It reaches copies by asking the modules that gave
them (`COPIES`, each `Copy` its place and `Owner`, answered per copy
with a `Reached`); with none answering, each is unreached.

### Listed for `lnk secret list`

`held` gives the secrets kept for you as `lnk secret list` shows them:
where each is kept, where it's given and where a copy was left, never
its value. One no longer kept here is listed by the copies left
elsewhere. A store that can't be read, or a keeper that doesn't answer,
is one entry saying so.

## The keeper

The keeper runs as `lnk-secrets` (`KEEPER_USER`), which holds secrets
and runs nothing else, and answers on `/run/lnk-secrets/keeper.sock`
(`SOCKET`) and, for sealed routes, `/run/lnk-secrets/secret.sock`
(`ROUTE_SOCKET`). `here` says whether there's a keeper on this machine,
and `running_as_keeper` whether this process is it.

### Its protocol

A call to whoever holds your secrets is a `Request`, answered with a
`Reply`; `Holder` is who holds them, the keeper or your own file. The
keeper never takes your file: while it holds a secret, a keeper here is
refused (`yours_held`).
`serve` and `serve_with_routes` are the keeper's side. A debug build's
tests point it at a keeper of their own with `LNK_TEST_KEEPER`
(`TEST_KEEPER`).

#### Its Limits

Each call has one deadline, 30 seconds from its accept, for its every
read and write, so a caller writing a byte at a time holds the keeper
no longer. A store is at most 8 MB, and `Store::save` refuses to write
one larger. `remove_user` asks the keeper, as root, to remove another
user's store and the note of whose it was (`remove_user_store` on the
keeper's side), before that user is removed; any other caller is
refused.

#### Its version

On the socket, each call is one line of JSON each way,
`{"protocol", "request"}` and `{"protocol", "reply"}`, each stating the
keeper protocol's version its sender speaks. The keeper is a system
service, upgraded apart from your `lnk`, so each side refuses the other
when it speaks a version older than the oldest it accepts, naming the
upgrade. [The versions](../../../../../../../../packages/version/docs/more/libraries/types/version.md#protocol-versions) are declared in `keeper/protocol.toml`.

### At rest

`at_rest::has_tpm` says whether there's a TPM. `at_rest::write` seals
first, then atomically replaces and syncs the file and its directories;
without a TPM the file is the keeper's private fallback, never the
working user's. `at_rest::read` opens a private file, and migrates
plaintext when asked. `at_rest::require_sealing`, which the keeper calls
when its unit says `--sealed`, makes a write that finds no TPM fail
rather than write plaintext. The payload is sealed at the envelope's
schema version and opens from its oldest (`open_sealed_within`), so a
change to it is a step of that schema.

## The secret route

The route adds a sealed secret to its service's calls on their way out
of a sandbox, served at `/_link/secret/` on the proxy (`PREFIX`) by `lnk
secret bridge` (`bridge`). `route` builds one from the secrets handed
with their values and those granted by name, none named twice.

### How a secret is given

`Sealing` is how a sealed secret is given, set with its value: the one
host it's sent to, over HTTPS, the header it goes in, and that header's
value. `SecretRule` is where a secret may go and how, which a module
that owns the service gives (a connection module's `rule --json`) and the
proxy enforces, knowing nothing of the service: its paths
(`SecretPath`, `SecretRest`) and header (`SecretHeader`).

### A secret in a socket's first message

Some services take their secret in a WebSocket rather than a request.
A WebSocket is a connection a program opens with one HTTP request and
then keeps open, each side sending messages, called frames, whenever
it likes. Discord's Gateway is one: a bot logs in by sending its token
in the first message, a JSON object, and the socket stays open while
it runs.

#### The socket a rule names

A rule's `socket` (`SecretSocket`) covers such a service. It names the
socket's one host, which may differ from the rule's `host` for the
service's requests, and the path on the proxy that opens it. It also
names a JSON pointer, the path into the first message where the secret
goes (`/d/token` for Discord).

#### The secret in place of its placeholder

The program opens the socket at
`/_link/secret/<name>/<path>` with a placeholder at that pointer. The
bridge opens the same socket at the host over TLS, passing the opening
request's headers through so the service answers the program's own
handshake. It then sends the program's first message with the secret in
place of the placeholder. A first message without the placeholder at
the pointer ends the socket, the secret unsent. After that first message
the bridge reads no frame: each crosses as it came, both ways
(`lnk_secret::socket`).

### Names on the proxy

`proxy_name` gives a sealed secret's name on the proxy (`stripe-key`
for `STRIPE_KEY`), which `check_secret_name` holds to 1 to 40 lowercase
letters, digits and `-`, not starting with `-`. `sealed_url_var` gives
the variable what runs calls its service at, `LNK_SEALED_<NAME>_URL`,
and `url` that address on a proxy's port.

### Placeholders

`placeholder` gives what a program holds in place of a secret: a tag,
none of the secret, after the part a rule says isn't secret, which a
client may read (`secret_prefix`).

## Bindings

[A workload's binding](../../../../../../sandbox/bridge/binding/docs/tooling/sandbox/more/bridges/binding.md) proves its calls to every bridge.
`keeper::RouteRevoker` is the `Revoker` a keyring is given when it
unbinds a workload: it asks the keeper to revoke the routes granted to
that binding (`unbind_route`) before the key is removed.

## Isolated TPM Tests

`LNK_TEST_TPM` substitutes the TPM (Trusted Platform Module) utility for an
isolated storage fixture. `LNK_TEST_TPM_RUN` selects that fixture's private
operation folder. Both are debug-only inputs and ignored by release builds.
These stand-ins exercise storage orchestration without proving hardware sealing
or recovery on another machine.
