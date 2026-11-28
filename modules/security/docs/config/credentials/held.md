# Held Credentials

The credentials you hold for other services, such as a model provider's API key or a cloud account's keys, which Local Link keeps and uses but never mints. Each is a secret kept as [Secrets](../../../secret/docs/config/secrets/README.md#where-a-secret-is-kept) explains, and listed without its value.

```sh
lnk secret list
```

## Each, Where It Lives and Who Reads It

What each held credential is, where Link keeps it, and which program
reads it. A few secrets held beside them authorize nothing, and each of
those says so.

### A harness's token

A native harness can keep an endpoint or gateway token across starts.
Its adapter lists where it is held and what it authorizes without
printing its value, separately from the model keys the harness uses.

### A model's key

The provider's variable, `LNK_FALLBACK_CREDENTIAL` for a fallback, or asked
for by [`lnk agent start`](../../../../agents/agent/docs/agents/README.md#start-your-agent). It lives in your secrets, as the agent's own,
kept and granted by the agent's ID and its use, and the harness's own `.env` (DeepSeek Harness's store of
keys). The harness reads it, or [the sandbox's proxy where it adds the
key outside](../../../../agents/agent/docs/agents/models/agent/security.md#secrets).

### A connection's token

From [`lnk agent connect`](../../../../agents/agent/docs/agents/connections/connect/README.md#choose-an-app): its variable, or asked for. It lives in your
secrets as the agent's own, by its ID and the connection's field, as a model's key does, and
the connection's bridge reads it, outside the sandbox.

### A tool's key and a remote tool's headers

A tool's key (a search service's, as `BRAVE_API_KEY`) lives in the
tool's settings, which [`lnk sandbox tool`](../../../sandbox/docs/tooling/sandbox/reach.md#call-a-tool-that-runs-outside) writes, mode 600, and the tool sends it to
its service alone. A remote tool's headers (`lnk sandbox tool add
--header-file`) live in the tool's settings, mode 600, read by the tool
outside every sandbox.

### An export's headers

From [`lnk measure export set --header-file`](../../../../tooling/measure/docs/tooling/measure/export/README.md#set-where-looks-go): they live in the watcher's
settings, mode 600, and the watcher sends them to its endpoint alone.

### A cloud account's keys

From [`lnk cloud connect`](../../../../cloud/cloud/docs/services/cloud/guide.md#connect-a-cloud-account): they live in the Keychain, else
`accounts.toml`, mode 600, and [`lnk box`](../../../../compute/box/docs/services/boxes/README.md#boxes) and [`lnk storage`](../../../../storage/storage/docs/services/storage/README.md#storage) read them.

### A bucket's keys and encryption password

A bucket's keys are its credentials, which open the bucket. Its
encryption password is a secret that authorizes nothing: it encrypts
what you push with `--encrypt`. Both live in the Keychain, else
rclone's settings, read by `lnk storage`, and by the files bridge for an
agent's folder.

### A storage token that refreshes itself

The storage bridge's: a token of minutes and what gets the next, kept in
the bridge's memory and handed to the bridge outside every sandbox, so
the bridge never holds a cloud's own sign-in.

### A secret you keep

A secret you keep with [`lnk secret create`](../../../secret/docs/config/secrets/README.md#create-a-secret)
is a credential when its value authorizes something, such as an API
key, and a secret all the same when it doesn't. It lives in the Keychain on a Mac, and on Linux in the
keeper's store, else `~/.lnk/safe/secrets/secrets.toml`, mode 600, with how it's
given (a sealed one's host) and where it's granted. Only what its grants
name reads it: [an agent granted it](../../../secret/docs/config/secrets/README.md#grant-a-secret), sealed, through its sandbox's proxy,
which adds it for its host alone.

#### When saved secrets cannot be read

This entry is a status, not a secret. The credential viewer reports an unreadable store as a status without
claiming to know its contents. It exposes no secret value and keeps the
failure distinct from an empty store or a known individual credential.

### Link's SSH key for boxes

Link makes an SSH key for reaching the boxes it creates and keeps the
private half on this machine. Boxes trust it only with a short-lived
certificate the box SSH secret signs; credential views describe its
location and access without printing it.

#### The box SSH secret

The secret half of this machine's SSH certificate authority, which
signs the certificates boxes trust, kept in the Keychain on a Mac, else
in a file of the user's alone. The public half is installed on each
box at its first boot; credential views describe where the secret is
kept without printing it.

## Where They're Kept

A credential is kept as every secret is, starting from [Where a Secret
Is Kept](../../../secret/docs/config/secrets/README.md#where-a-secret-is-kept).
This section says where the credentials other modules keep differ.

### Credentials the Secret Module Keeps

The secret module keeps the credentials you add with [`lnk secret create`](../../../secret/docs/config/secrets/README.md#create-a-secret)
and those it's given for what you run, such as a model credential and
a connection's token. Each is in the Keychain on a
Mac, and on Linux with the [Secret Keeper](../../../secret/docs/config/secrets/keeper/README.md),
or else in a file of your own. The keeper hands each one only where a
grant names it, and a sealed one reaches only the run's proxy.

### Credentials Other Modules Keep

A module that keeps a credential of its own keeps it in the Keychain or
the Safe, out of every sandbox, and names it for [`lnk secret list`](../../../secret/contract/docs/config/secrets/more/contract.md#listed-for-lnk-secret-list).

#### Cloud Accounts' Settings

Cloud accounts separate `settings` (regions, profiles, project names
and credential-file selectors) from `env` (guarded credential values).
The cloud CLI is explicitly given both at its child boundary for
signing, and selectors contain paths, never the credential file's
contents. Cloud CLIs and rclone receive the credentials they need at
their child boundary, as [No Child Carries One It Wasn't Given](../../../secret/docs/config/secrets/README.md#no-child-carries-one-it-wasnt-given)
describes.

#### Not Built Yet

On Linux, a cloud account's keys, a bucket's keys and a tool's headers
remain files of your own user in Link's settings, not the keeper's. On a
Mac the first two are in the Keychain, and a tool's headers remain a
file. Ordinary-box work cannot read those controller files without a
grant. An explicitly requested root stage remains a trusted exception
outside confinement.

### On a Box

A box's keeper seals its store to the box's TPM where it has one, and
the files other modules keep are on a disk its cloud encrypts. [Keeper on a
Box](../../../secret/docs/config/secrets/keeper/README.md#keeper-on-a-box)
says how each works and what a box without a TPM keeps.

## How They're Kept

What holds for every held credential, beyond the rules every secret
keeps.

### Never on a command line, never in output

A held credential reaches the program that uses it as a file or on
standard input, never as an argument, and no describing command prints
it.

### A connection's token sent from the account service

It's sealed, as you ask for it, to the receiving machine's [sealing
key](installation.md#the-sealing-key), for that machine's agent (`<machine
id>/<agent>`) and connection, so the service never keeps it readable,
and hands over only what's sealed. A machine whose `lnk` hasn't said
what to seal to is asked no connect.

#### On the machine

The machine opens it, hands it to `lnk agent connect` on its pipe, and
it's kept as a connection's token is.

### Given only where it's granted

A secret you keep goes only to what [`lnk secret add`](../../../secret/docs/config/secrets/README.md#grant-access-to-a-secret) gives it to, and the
grant is kept with the value, never in a file that names it, so editing
a file never widens it. An agent's grant is bound to its ID, never its
name.

### Gone with what it's for

An agent's keys go when it leaves the machine. A cloud account's go when
the account is disconnected, and a bucket's with its section. A secret
you keep goes with [`lnk secret delete`](../../../secret/docs/config/secrets/README.md#an-agent-that-leaves), which also reaches each box a
move sent it to, as the move noted before sending it. [A box it can't
reach stays noted](../../../secret/docs/config/secrets/README.md#copies-elsewhere), and `remove` fails until it reaches it.
