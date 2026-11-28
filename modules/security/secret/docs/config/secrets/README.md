# Secrets

> Secrets Go Only Where You Grant Them

A secret is any value Local Link keeps private for you, such as an API
key, a token or a password. Keep one on this computer, then grant it
only to the agents and environments that need it, which use it without
ever seeing its value.

<!-- quickstart -->

## Create a Secret

Enter your Stripe key when prompted. It is sealed for `api.stripe.com`,
so an agent can use it there without receiving its value.

```sh
lnk secret create STRIPE_KEY --internet-host api.stripe.com
```

## Grant Access to a Secret

Give a running agent named `main` permission to use the secret.
Start the agent first if it does not exist yet.

```sh
lnk secret add STRIPE_KEY --to agent:main
```

## Inspect Your Secrets

List stored secrets or inspect one secret’s grants and host restrictions.
These commands never print the secret value.

```sh
lnk secret list
lnk secret show STRIPE_KEY
```

## What a Secret Is

A secret is any value kept private, because whoever reads it could
misuse it. Some secrets open something, such as a model provider's API
key or a bot's token. Others open nothing and are private all the same,
such as a bucket's encryption password, which encrypts your files, or
the sealing key a machine opens a sealed transfer with. Local Link
treats every one the same way, whatever it opens.

### Credentials Are Secrets That Authorize

A credential is a secret that authorizes: holding it lets someone do
something, such as call a model provider, post as a bot or reach a box.
A credential is named by whose it is, as a box's certificate is this
computer's credential for that box. Every credential is a secret, so everything on this
page holds for it. [Credentials](../../../../docs/config/credentials/README.md)
builds on this page with what a credential adds: who holds it, what it
opens, and how it's issued, rotated and revoked.

## Where a Secret Is Kept

A secret Local Link keeps is control, kept in a
[Safe](../../../../../storage/safe/docs/config/safe/README.md) that no
run reaches by any path. The secret module keeps the secrets you add
and those it's given for what you run. Another module keeps only its own secrets, such as a cloud account's
keys, and names them for [`lnk secret list`](../../../contract/docs/config/secrets/more/contract.md#listed-for-lnk-secret-list).

### On a Mac and on Linux

On a Mac, each value is in the Keychain (`link`, `secret/<id>`).
On Linux, they're kept by the keeper, a user of their own (`lnk-secrets`)
that no program of yours runs as, once you've made it; until then, and
if you say no, in `~/.lnk/safe/secrets/secrets.toml`, mode 600, which any
program you run outside a sandbox can read. Either way the store holds how each is given
and where it's granted too.

### Store Shape

The store keeps each secret under its ID, the IDs of those deleted,
which are never given again, and the copies a move left of an agent's
own secrets no longer kept here. Each entry says its `kind`: `named`,
kept by `lnk secret create` under its `name` as
[Who a grant names](#who-a-grant-names) shows,
or `own`, a service's own, which has no name:

```toml
# ~/.lnk/safe/secrets/secrets.toml
schema = 1
retired = ["r7s8t9u0v1w2"]

[entry.m5n6p7q8r9s0]
kind = "own"
to = [{ principal = "service:s1t2u3v4w5x6", action = "secret.secret.read", resource = "secret:m5n6p7q8r9s0", workload = "agent:a1b2c3d4e5f6", name = "main", as = { kind = "own", purpose = "model" } }]
copies = [{ name = "aws-1", resource = "box:b7c8d9e0f1g2" }]

[left.x1y2z3a4b5c6]
principal = "service:s1t2u3v4w5x6"
as = { kind = "own", purpose = "fallback" }
copies = [{ cloud = true, name = "cloud-1" }]
```

A copy on a box names the box's machine as `resource = "box:<id>"`, the
form [`lnk box list --json`](../../../../../compute/box/docs/services/boxes/guide.md#list-your-boxes) writes. On Linux each entry holds its `value`
too. A store an earlier `lnk` wrote, each secret under its name, is
refused, never read or converted: remove it, then add your secrets again
with `lnk secret create`, and start your agents again for their own. On
a Mac the refusal names that store's Keychain items, `link:secret/<name>`,
which nothing reads, for you to delete with Keychain Access.

### Nowhere Else on the Disk

Secrets are kept nowhere else on the disk. An agent's proxy's routes,
`~/.lnk/safe/sandboxes/<agent>-<harness>.routes.json` (600), name its
sealed secrets, its connections' tokens and its model's key, and hold
none: at each call, its bridge reads the one it adds from the store,
only while it's granted the agent, so a secret or grant removed stops
at the next call.

### Secret Keeper

On Linux, the [Secret Keeper](keeper/README.md) is a system user of its
own that keeps your secrets, so no program running as you reads them
from disk. Your first `lnk secret create` there asks once whether to
make it, and its page says what making it does, what a no keeps, and
how it holds secrets on a box.

### Listed

`lnk secret list` lists each secret the keeper keeps, never its value, and says so
when the file can't be read. It lists the secrets every other module keeps
on this machine too, most of them credentials such as your Link login, and
`--to agent:work` lists only the secrets given to that agent. Where each
credential is kept and which program reads it is in
[Held Credentials](../../../../docs/config/credentials/held.md).

## A Secret's Life

A secret is kept, sealed for a host, granted, carried where it moves
and taken back, in that order, and each step has a section below.

### What You Need

You need the secret module: [`lnk mod add secret`](../../../../../../docs/more/modules/guide.md#add-or-remove-a-module), or any `lnk secret`
command offers it. Use a value of yours when prompted. [Environment Secrets](environment.md) walks through giving a secret to an environment you run, including its complete TOML file. [Environments](../../../../../compute/env/docs/services/environments/guide.md#variables-and-secrets)
explains how a run receives its sealed secrets.

### Keep a secret

`lnk secret create` keeps a secret: its value, asked without echo or read
from its input's first line, and how it is given. It is given to no one
yet. It prints the secret's ID, `secret:<id>`, which no other secret here
is ever given, and every command takes the secret's name or that ID
(`lnk secret show secret:k3m4n5p6q7r8`). Creating a secret again
replaces it whole under a new ID: its value, how it is given and every
grant. With `--json`, `lnk secret create` and `lnk secret delete`
answer `{"resource": "secret:<id>"}`, and delete adds where its copies
went and where they stay.

#### Keychain Item of a Replaced Secret

On a Mac, a secret created again leaves the Keychain item of the one it
replaced only when the Keychain won't delete it. The new secret is kept
all the same, and a note names the item, `link:secret/<id>`, which holds
a value nothing reaches, for you to delete with Keychain Access.

### Seal a secret for one host

A run, such as a program in a sandbox, is given a secret in one of two
ways. A sealed secret is bound to one host. The run is told a
placeholder, and this module's bridge, outside the sandbox, adds the
value to each request to that host as it leaves through the sandbox's
proxy, so what runs never holds it. A plain secret is handed to the run
as a variable, to read, so it suits only a run you trust with its value.
Each way below seals one.

#### Use a Bearer token

For a service that accepts an `Authorization: Bearer` credential, give
its one intended host with `--internet-host` when creating the secret,
as [Create a Secret](#create-a-secret) does for Stripe.

#### Use Basic authentication

For a service that accepts a token as the password in HTTP Basic
authentication, set its username and intended host. GitHub uses
`x-access-token` as that username.

```sh
lnk secret create GITHUB_TOKEN --internet-host github.com --basic-user x-access-token
```

#### Use a custom header

For a service that accepts its credential in another header, name that
header and the intended host. Create this Anthropic API key rule, then
grant it to the existing `main` agent.

```sh
lnk secret create ANTHROPIC_KEY --internet-host api.anthropic.com --header X-Api-Key
lnk secret add ANTHROPIC_KEY --to agent:main
```

#### Use Another Value Format

For a service whose header holds the secret inside other text, such as
`token <secret>`, give that text with `{}` where the secret goes. Left
out, the value is `Bearer {}` for `Authorization`, `Basic {}` with
`--basic-user`, and the secret alone in any other header.

```sh
lnk secret create GITEA_TOKEN --internet-host gitea.example.com --format 'token {}'
```

#### Host and Header Rules

How a sealed secret is given (its host, header and format) is set with
its value, never by the file that names it. The proxy adds it to any
call to its host, on any path and with any method: whatever runs can use
it there as fully as the secret allows, so seal it for the one host it's
for, with the narrowest secret that host offers.

### Grant a secret

`lnk secret add` gives a kept secret to an agent on this computer, once
for each `--to agent:<name>`. The flag names a workload by its kind and
name, and the module that keeps that kind says which service it acts
as. The secret module gives a secret to an agent alone: an environment or a job names no grant here, since [an environment
you run](environment.md) acts as you and is given what you approve for it with [`lnk env approval`](../../../../../compute/env/docs/services/environments/approval/README.md#add-an-approval).

#### Who a grant names

A grant names who acts, never what runs: [the service the agent acts as](../../../../../agents/agent/docs/agents/more/access.md#an-agents-service),
`service:<id>`, which Link mints with the agent under an ID of its own.
Each grant is kept as a principal, an action and the secret it gives:

```toml
# ~/.lnk/safe/secrets/secrets.toml
schema = 1
[entry.k3m4n5p6q7r8]
kind = "named"
name = "STRIPE_KEY"
sealed = { host = "api.stripe.com" }
to = [{ principal = "service:s1t2u3v4w5x6", action = "secret.secret.pass", resource = "secret:k3m4n5p6q7r8", workload = "agent:a1b2c3d4e5f6", name = "main", as = { kind = "variable" } }]
```

The action follows how the secret is kept: `secret.secret.pass` for a
sealed secret, which the proxy adds, and `secret.secret.read` for a
plain one, whose value is read. This module checks the grant itself.
`resource` is the secret it gives, and `as` how, by its `kind`:
`variable`, as a variable of the secret's name, or `own`, a service's
own for its `purpose`. `workload` and `name` are kept to show, as
`main (agent:a1b2c3d4e5f6)`, so a name is never mistaken for an ID.
`lnk secret remove` stops giving it, and the secret stays kept.

#### List Grants as Kept

[`lnk secret list --json`](../../../contract/docs/config/secrets/more/contract.md#listed-for-lnk-secret-list) writes each grant as kept, its
service under `principal`.

#### Grant a Secret to an Agent

An agent acts as its service from its first save, so [`lnk agent create`](../../../../../agents/agent/docs/agents/start/create.md#create-on-a-hosting-account)
or the first [`lnk agent start`](../../../../../agents/agent/docs/agents/README.md#start-your-agent) of `main` comes before a grant. An agent is given a secret sealed only, for its host
(`--internet-host`), and is told a placeholder, never the value. An agent made later with the name
gets none of it. An agent on a box is granted its secrets there (`lnk
box ssh <box>`).

#### Secrets for a Module's Workloads

What a module keeps may be given a secret plain: with no `--internet-host`, it's
kept plain and given as that module gives it; with `--internet-host`, sealed, as
an agent's is. [An environment's or a job's ID](../../../../../compute/env/docs/services/environments/guide.md#variables-and-secrets) belongs to its file where
it is, kept by its module, never in the file: a copy of the file, or the
file moved, is another environment or job, given nothing.

### Your agents' own credentials

An agent's model credential, its fallback's and its connections' tokens are kept
here too, granted to it: `lnk agent start` and [`lnk agent connect`](../../../../../agents/agent/docs/agents/connections/connect/README.md#choose-an-app) add
them. Each has an ID of its own and no name: it is found by the agent's
service and its use, so `lnk secret list` shows it as `service:<id>'s
own, its model's key`, and a rename of the agent changes none of it. Such a
secret is never granted by hand.

#### Removed or moved with the agent

When the agent is removed they go with it. When it moves, with [`lnk agent move`](../../../../../agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud), they go with it too, sealed to the machine it goes
to, and are kept there as they were here. A key still where Link kept it
before (the agent's settings, or the Keychain's `agent/<agent>/…`) is
never read nor removed: the agent's start names it and how to give it
again.

#### An agent that leaves

A secret granted to an agent that leaves this computer stays here only
while something else is still granted it, or while it is a model credential
retained for a host that supplies the agent's models. A retained key
has no live agent grant and `lnk secret delete` still deletes it.
What you added with
`lnk secret add --to` doesn't go with the agent: grant it again
where the agent runs.

### Look at one secret

`lnk secret show STRIPE_KEY` says how one secret is given and where it is
granted, never its value. [`lnk issue create`](../../../../../../core/cli/docs/tooling/issue.md#report-a-bug) asks it for the state of a secret
named in a failure, so a bug report holds it scrubbed.

### Take a secret back

`lnk secret delete STRIPE_KEY` removes its stored value and grants here.
An agent's named route checks the store on each call, so its next call
is refused. A request already dispatched can finish, and a run handed
a value directly retains that value until it ends. Revoking Link access
does not revoke the upstream credential; rotate it at its issuer when
that is needed.

#### Copies elsewhere

When a move sends an agent's keys to a box or a cloud account, this
computer notes the place for each key first. It does this before
anything is sent, so a move cut short is noted too. `lnk secret list`
shows where each secret has copies. `lnk secret delete` asks the
module whose moves made them, the harness module, to reach each place
(its question `secret.copies`).

#### A host that supplies its models

When a host supplies its own models, the model credentials stay at the source
machine and remain noted there, including on a computer controlling a
move from a box. Connection tokens still follow the agent. Each copy is
noted by the secret's ID, the agent's service and its use, so the two
are never mistaken for each other.

#### Secret Copies on a Box

The owning move adapter removes that agent's own secret for that use
through the [remote copy interface](more/protocol.md#remove-a-remote-copy).
The box keeps it under an ID of its own. A box that's gone took its copy
with it, and so did one made again under its name since, which is never
touched. A box without the secret module (one readied before boxes got
it) is named, with how to add it: `lnk box run <box> -- module add
secret`.

#### Box You Can't Reach

A box it can't reach (stopped, or offline) stays noted. `lnk secret
list` shows it as left there, and `delete` exits non-zero. Run `lnk
secret delete secret:<id>` again once the box is up, and it reaches it.

#### On a cloud account that hosts agents

The key is removed when the agent there is removed or moves away.
`delete` says so, and the note stays until the agent moves away, or
until `delete`, signed in as the account the agent went there as, finds
that the account no longer lists it.

#### Once an agent leaves

Once an agent leaves a box or an account, the keys it took are gone
from there, and so is the note. Each agent's notes are its own: an
agent made later with a removed agent's name never changes them.

### Deleted on Uninstall

[`lnk uninstall`](../../../../../../docs/more/modules/guide.md#uninstall) deletes what the keeper keeps for you with the secret
module's part. It first removes each copy a move left on a box, as `lnk
secret delete` does, and while one can't be reached it stops and keeps
your secrets, so the note of that copy is never lost.

### Fix a Failed Secret Command

When `lnk secret` refuses a grant, leaves a copy on a box or a cloud
account, or can't reach its keeper, [Troubleshooting](troubleshooting.md)
says what to do.

## How Local Link Handles Every Secret

These rules hold every secret any of Local Link's programs holds,
credentials included, whichever module keeps it. A secret Local Link
handles in any other way is a bug.

### Never Printed

Ordinary status, error and log output never carries a secret, whether
text or JSON. A command that manages secrets returns a value only when
that is its explicit purpose, through a pipe or a protected file.
[`lnk secret list`](../../../contract/docs/config/secrets/more/contract.md#listed-for-lnk-secret-list)
lists every secret this machine holds without its value, so nobody digs
through hidden folders for their own.

### Never on a Command Line

Every user of the machine sees a command line. A secret comes from
standard input, a file, a variable or the Keychain, and no flag takes
one. The command line refuses a command whose flag named like a secret takes
a value, a file's path aside.

### Carried Only by What's Made to Carry It

A secret travels over SSH (a move's stream) or TLS (a service, a grant
read once), and to a program on a pipe or standard input. It never
travels on a command line, through the relay, in a bucket or in a log.

### Sealed When It Crosses Machines

A secret that must reach another machine crosses only sealed, to a key
the receiving end makes for that one transfer and says first. The
receiving machine drops that key when its receiving process ends, so a
captured transfer opens nothing afterward. [Secretive](../../../../../../packages/secretive/docs/more/libraries/files/secretive.md#sealed)
names the encryption and how a sealed stream is cut into authenticated
records.

#### For a Machine Not There Yet

What must wait for a machine that isn't there yet, such as a
connection's token you connect in the browser, is sealed instead to a
key the machine keeps for it, its
[sealing key](../../../../docs/config/credentials/installation.md#the-sealing-key).
Short bearer tokens made to be shown, to one audience over TLS, are
the one exception, by purpose.

### No Child Carries One It Wasn't Given

A program Link runs gets Link's environment without any variable named
like a secret's, and without any Link read a secret from. A name is a
secret's when a word of it is `TOKEN`, `KEY`, `SECRET`, `PASSWORD`,
`PASS`, `AUTH` or `CREDENTIALS`, or it ends `DATABASE_URL`, but never
when it ends `_FILE`, `_PATH`, `_DIR` or `_SOCK`. A variable read as a
program starts, on one thread, leaves its environment.

#### Link's Own Programs

Link's own programs (`lnk`, a module, a tool) are handed every secret
the program that started them holds, [on a pipe](more/protocol.md#secrets-a-program-is-handed).

#### Programs That Read Their Environment

A program that reads a secret from its environment and nowhere else
gets it there, each a reviewed place in Link's code, with why. A
cloud's command line (`aws`, `gcloud`, `rclone`) gets its sign-in. A
tool's server, added with [`lnk sandbox tool`](../../../../sandbox/docs/tooling/sandbox/reach.md#call-a-tool-that-runs-outside),
gets its environment as you gave it. The sandbox's own stages give the
program the environment their caller gave it.

### Held by a Process Safe to Hold It

Every one of Link's programs makes itself so first, as it starts. It writes no core dump and isn't dumpable, so no
other process of its user reads its memory, its environment or its
descriptors. A program it runs is dumpable again once it starts. It
locks its memory out of swap where it may lock all of it, never by
raising a limit a program it runs would inherit. A debug build with
`LNK_SECRET_DUMPABLE` set is left dumpable for a debugger, and a release
build never reads it.

### Held in Types by Where It May Go

In Rust, every secret is in one of the `link-secretive` library's
types, which `link_core` re-exports, and a wrong way doesn't compile.
An `OpaqueSecret` signs and checks, with no way to its bytes. A
`Secret` is a value another program or service must be given, read only
by asking for it by name. A `Sealed` is either one encrypted to one
recipient. None prints, clones or serializes what it holds, and each is
zeroed when dropped, as [Secretive](../../../../../../packages/secretive/docs/more/libraries/files/secretive.md#its-types)
explains.

#### Every Place Is Reviewed

A new place a secret is read, written as its value, or given to a child
is checked before it's accepted, and a review sees it. A `Secret` is
written as its value only where it's kept, a file of mode 600 or the
keyring. A field named like a secret holds one of the types, or is
reviewed with what it holds instead, such as a public key. A copy of an
exposed value, a redacting `Debug` written by hand, and `zeroize`
outside the library are never accepted.

## Build on Secrets

The keeper's storage, grant ownership, and the choice to seal agent
secrets for their destination have reasons that [Secret
Decisions](decisions.md) explains. The [Secrets
Contract](../../../contract/docs/config/secrets/more/contract.md) defines
stores, grants, and keeper requests for callers. Programs receive their
permitted secrets without placing values in ordinary process arguments or
inherited environment variables, as the [Interface](more/protocol.md)
explains.

<!-- docs-tree:start -->

## Docs

### Pages

- [Environment Secrets](environment.md): Give an API key to an environment you run without putting its value in the environment file
- [Secret Keeper](keeper/README.md): The keeper is a system user that keeps your secrets, and no program of yours runs as that user

### More

- [Interface](more/protocol.md): How a program is handed the secrets it may use, never through its environment
- [Decisions](decisions.md): Why `lnk secret` works the way it does
- [Troubleshooting](troubleshooting.md): What to do when [`lnk secret`](README.md) refuses a grant, leaves a copy behind, or can't reach its keeper

<!-- docs-tree:end -->
