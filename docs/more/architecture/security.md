# Security

How Link protects the people who use it, what it doesn't protect
against, and where each part's details are.

## Report a vulnerability

[Report it privately](../vulnerabilities.md):
GitHub's "Report a vulnerability" button, or **so@local.link**. Never in
a public issue.

## Each part's page

Each part's guarantees and gaps are on its own security page:

- Agents: [Security](../../../modules/agents/agent/docs/agents/more/security.md): the harness, its sandbox, moving an agent
- Sandbox: [Security](../../../modules/security/sandbox/docs/security/sandbox/security.md): what an agent's traffic may reach
- Models: [Security](../../../modules/agents/model/docs/agents/models/security.md)
- [File Security](../../../packages/files/docs/more/libraries/files/files/security.md): control documents, path aliases and pinned handles
- Storage: [Security](../../../modules/storage/storage/docs/services/storage/security.md): your files, share links
- Clouds and boxes: [Security](../../../modules/compute/box/docs/services/boxes/security.md)
- Releases: [Security](../modules/security.md): the core and its updates

## Who is involved, and what they can do

Who takes part, what each is trusted with, and what each must not be
able to do. The user runs `lnk`, trusted with their own agents, clouds
and boxes. A chat sender reaches the harness through their chat, and
`lnk` reaches the user's clouds from the user's machine:

```
Telegram, Slack, Discord, Signal sender ──▶ Telegram, Slack, Discord, Signal ──▶ harness (sandboxed, on the user's machine) ──▶ model provider, the internet
                                  lnk storage (user's machine) ──HTTPS──▶ the user's clouds
                         lnk cloud, lnk box (user's machine) ──HTTPS, SSH──▶ the user's clouds, their boxes
```


### Harness

The harness may be steered by a prompt injection, from a web page, a
message, or a file it reads. Its sandbox grants determine which data,
programs, and services it can reach. Protected Safe folders stay outside
that sandbox even when the owner grants broad access to data. Named
secret routes add credentials outside the sandbox rather than handing
their values to the harness. The
[agent security guide](../../../modules/agents/agent/docs/agents/more/security.md)
explains the defaults, wider grants, and limits.

### Chat sender

A sender on Telegram, Slack, Discord or Signal is refused unless they
are the user or member id paired with the bot or app, or in a chat the
owner shared the agent with: there they can make it do all it can, and
are never taken as the owner. Only the owner shares a chat, unshares it
or approves what the agent asks.

### Module from outside Link

A harness's adapter from its own repository, signed by its own key, runs
as the user, like any program they install. It can't install an update
not signed by the key the user trusted, or take one of Link's modules'
names.

### Vendor installers

A harness's own installer and rclone's releases are trusted with
installing their program, and can't write outside the harness's home or
change rclone. They run sandboxed, and rclone is installed only if its
checksum matches the one pinned in Local Link's source.

### Cloud providers

Cloud providers, and anyone with the account or bucket, hold the user's
files and boxes. They can read files not pushed with `--encrypt`, and
run the user's boxes, so everything on one, its own `lnk` login
included. A box holds no key to the user's own machine, and is never
handed the user's SSH agent.

#### What `clean` keeps

They can't make `clean` delete a file here that has no good copy, short
of deliberately forging MD5 checksums as the provider; on S3, `clean`
[reads back any copy](../../../modules/storage/storage/docs/services/storage/security.md#what-clean-guarantees) whose MD5 its uploader set.

#### A shared memory engine

A shared memory engine proves each agent's embedding requests with a
purpose-bound credential on a Unix socket. The receiver resolves the
owner from the kernel peer and current binding, refusing an asserted
owner, another slot's user and a replaced binding. [The proof](../../../modules/security/docs/config/credentials/agent.md#a-memory-embedding-proof) gives its
receiver no agent key.

## What the design addresses

The threats the design is built against:

- anonymous internet traffic;
- a compromised releases repo or its token;
- a harness turned against its user by what it reads;
- a stranger messaging the user's bot;
- a compromised harness installer;
- a cloud that loses or corrupts copies.

### The network and local users

TLS protects every connection except loopback against a network attacker
between the parties. Other local users on the same machine are kept out
of Link's files (mode 600 and 700) but can see command lines, so secrets
are asked for or read from variables, never taken as flags where that
can be avoided.

## Link's own calls

Every call Link's own code makes over HTTP goes out on one client,
`link_fetch::Client`: to the account API and to other services it uses,
such as a cloud API, a chat service, or a
release's files). What an agent fetches (a page, an MCP server) and a
model's provider have rules of their own, in the parts that call them.
Each call gets the same rules:

- **`https://` only.** Plain `http://` goes only to this machine
  (`localhost`, a name under it, or a loopback address). Anywhere else nothing is
  sent, so a credential never crosses the network in the clear.
- **Plain `http://` stays on this machine.** Its names are never asked
  of DNS: `localhost` and every name under it are `127.0.0.1` and `::1`,
  so a network's DNS can't answer `dev.localhost` with an address
  elsewhere. And it goes through no proxy, whatever `HTTP_PROXY` or
  `ALL_PROXY` say, since a proxy would carry it off the machine in the
  clear.
- **`https://` is any program's.** It uses the machine's DNS and
  proxies as any program does: TLS keeps a credential from them.
- **No redirect is followed.** An answer that sends the call elsewhere
  is an error, so a credential, or a call, goes only where it was sent.
- **An answer's size is capped,** 1 MiB unless a call says more (a
  share's list of files), and read so: a longer one is refused, never
  read whole first. A call takes 30 seconds at most unless it says
  another, and 10 to connect.
- **Only typed data crosses.** A request is a type, and an answer is
  read as one: a field it doesn't name, or a value of the wrong type, is
  refused, naming the call, never guessed at.
- **Errors map once.** 401 is signed out, 429 is try again later (with
  how long, when the service says), a 5xx is the service's fault. An
  error names the call by its method and URL without its user,
  password, query or fragment, and never repeats a credential or a
  value from an answer; what a service said is shown in printable text,
  at most 512 bytes of it.
- **Credentials go in as `Secret`s,** as `Authorization` headers marked
  sensitive, so nothing prints them.
- **Every call says it's Link's:** `lnk/<version>` as its user agent.

A client says where it's made when a call needs other than these, one
named rule each, so a review sees it:

- **A service that isn't Link's** (`outside`): its answers are read
  open, a field their type doesn't name passed over, since such a
  service adds fields without a new version. A call that reads some of
  a route's fields says so where it calls (`answer_part`).
- **A cloud's metadata server** (`metadata_server`), which has no TLS:
  plain `http://` to `169.254.169.254`, or to `metadata.google.internal`
  found at that address alone, never by DNS, carrying no credential.
  Any other plain `http://` beyond this machine is refused. Through no
  proxy.
- **Plain `http://` anywhere** (`plain_http`): an endpoint you named so, as with [`lnk measure export set --url http://...`](../../../modules/tooling/measure/docs/tooling/measure/export/README.md#set-where-looks-go), what it carries, its credential
  too, crossing the network in the clear as you chose. Through no
  proxy.
- **Public addresses alone** (`public_only`): an address someone else
  gave, a share's file, reaches no address of this machine or its
  network, and through no proxy, which would find the name itself.
- **Redirects followed** (`follow_redirects`): a release's public files,
  whose host moves them, each hop to `https://` alone. A call on such a
  client carries no credential.
- **A private certificate authority** (`trusting`), **`https://` alone**
  (`https_only`, this machine too), **no proxy** (`direct`), and **as
  long as a stream takes** (`streaming`): given up only once nothing
  moves for its stall's length, a body sent while it goes, an answer
  while it comes.
- **A URL that holds a secret** (a bot's token in its path) is named by
  what it is ("Telegram's getMe"), never by itself.

Every answer's bytes are held as a secret while they're read, and wiped
once they are. An HTTP client built anywhere else is a reviewed
exception, with why it isn't one of Link's own calls.

## Secrets in the Keychain

On a Mac, Link keeps its secrets in the login Keychain instead of its
settings files: the secrets [`lnk secret`](../../../modules/security/secret/docs/config/secrets/README.md#secrets) keeps (each agent's model credential and its connections'
tokens among them), cloud accounts' credentials, and the bucket keys and
encryption passwords [`lnk storage`](../../../modules/storage/storage/docs/services/storage/README.md#storage) writes. So they aren't in the plain
files that backups, dotfile syncs and copies of `~/.config` carry.

- **Through Apple's `security` tool.** Items are generic passwords of
  the service `link`, with accounts such as
  `secret/<id>` (each secret's ID, an agent's own alike),
  `cloud/<account>/<variable>`,
  `storage/<section>/<key>`.
- **Made by `security`.** `security` makes the items, so each item
  trusts it and reading one never asks. Link's programs aren't signed by Apple,
  so an item they made would ask again after every upgrade.
- **Any program of the user's can read one.** In exchange, any program
  of the user's can read an item through `security`, as it could read
  the file: the Keychain doesn't keep one of the user's programs from
  another.
- **No secret on a command line.** Secrets reach `security` on its
  stdin, hex-encoded, and each is read back after it's saved.
- **Fails closed.** A Keychain that refuses a secret, or can't be read
  because it's locked, stops the command with an error naming the item.
  Link never falls back to a file on a Mac. A settings file that holds a
  secret Link keeps in the Keychain is refused, never read.
- **Recorded.** Settings files record which secrets are in the Keychain:
  `accounts.toml`'s `keychain`, and `rclone-keychain.toml`.
- **Read only when used.** An agent's settings file keeps its Telegram
  bot's id, the token's public first part, so [`lnk agent list`](../../../modules/agents/agent/docs/agents/start/create.md#run-more-than-one-agent) and `lnk
  agent connect` name every agent's bots without reading their keys.
- **Moving out.** An agent's items go when it leaves the machine, an
  account's when it's disconnected, and a bucket's with its section.
  [`lnk uninstall`](../modules/guide.md#uninstall) removes only those of the parts you pick. It lists
  Link's items by name with `security dump-keychain`, never their
  secrets.
- **Stores follow their owner.** The Keychain package uses the login
  Keychain on macOS and private files on Linux. The
  [secret keeper](../../../modules/security/secret/docs/config/secrets/keeper/README.md)
  can hold Linux secrets under a separate user. Each component's
  configuration and security pages explain the store it uses.

## Privacy and logs

What each part keeps of who did what, and where it keeps it:

- **On the user's machine and nowhere else:** the harness's log (what
  it prints), the harness's own state in its home, the bucket manifests
  (every file's name, size and which cloud holds it), and the cloud
  accounts and boxes. `lnk` sends none of them to Local Link.
- **Who each part talks to.** The harness talks to its model provider
  and to Telegram itself, and `lnk storage` and [`lnk box`](../../../modules/compute/box/docs/services/boxes/README.md#boxes) talk to the
  user's clouds.
- **What a call leaves.** [`lnk model serve`](../../../modules/agents/model/docs/agents/models/local.md#serve-one-on-this-machine) logs nothing per
  request. The sandboxes' proxies log where each call went, never what
  it carried, which [`lnk sandbox log show`](../../../modules/security/sandbox/docs/security/sandbox/security/log/README.md#see-every-call-on-the-wire) shows.

## Known limitations

Each gap is followed by what limits it.

- **Link's signer is a GitHub environment's secret**, so repository
  admins, who can change the environment's rules, or a compromised
  first-party action in the signing job could sign. Only the
  `bundle-link` environment holds it: its jobs run only from `main`, and
  each waits for its required reviewer's approval.
- **A crate's build script could alter what gets signed**, since it
  runs in the build jobs. Locked, reviewed dependencies limit it. The
  builds restore no cache, so a poisoned cache can't alter them.
- **Signing stands apart from the builds,** which limits the two gaps
  above: actions are pinned and crates locked; signing runs in its
  own job with only `actions/*`, and signs only the build jobs'
  artifacts, after checking they hold exactly the expected files; the
  harnesses' installers, unpinned, run only on pull requests, never in
  a release's run.
- **Some secrets are in files.** Linux components can use protected
  user files or the secret keeper's separate store. On a Mac: the harness's `.env` (its connections' tokens, but for
  Telegram's when it's on the wire, and its model credential unless it's on
  the wire), a wired key or bot token for the proxy, Link's SSH key for
  boxes, and sign-ins rclone saves itself.
- **Those files are the user's alone,** which limits the gap above:
  they're readable only by the user, and secrets reach adapters on stdin
  and cloud CLIs in their environment, never on a command line.
- **[Keychain items are readable by any program of the user's](#secrets-in-the-keychain)** through
  `security`.
- **A module from outside Link runs as you.** [`lnk mod add --url <url>`](../modules/guide.md#add-or-remove-a-module)
  asks before trusting the offered key. Its adapter runs outside the
  sandbox. Link checks the component description and executable identity,
  and later releases must use the pinned signer. [An adapter launches
  its harness in the agent's sandbox](../modules/security.md#modules-from-outside-link).
- **A harness's installer may fetch some things unpinned**, such as a
  package's dependencies resolved at install time. Limited by: its
  adapter pins and checks the installer and the harness's own code, and
  everything runs over HTTPS, inside the sandbox, able to write only
  the harness's home, reaching only the hosts its adapter lists (a
  registry, GitHub) through its proxy.
- **One connected cloud key reaches both buckets**, your files' and your
  agent's. Limited by: the buckets are separate, and the agent's is
  always encrypted.
- **Link's SSH certificate authority for boxes signs for every box a
  computer reaches.** Limited by: its secret is in the Keychain on a Mac,
  else in a 700 folder readable only by the user, and a copied key
  alone reaches nothing an hour later.
- **A box's SSH port is open to the whole internet.** Limited by: it's
  key-only (Ubuntu's cloud images refuse passwords), and nothing else is
  open.
- **A new box's host key may be trusted on first contact**
  (`accept-new`) when its console doesn't show the host keys within 5
  minutes, or its adapter can't read it. That contact then trusts the
  network between you and the cloud. Limited by: usually the keys are
  read from the console and pinned first; otherwise the key is pinned
  after, in Link's own `known_hosts`.
- **With `--network.mode host`, a harness reaches more than it should.**
  On macOS it reaches every localhost port through `::ffff:127.0.0.1`,
  and a server listening on every address through the Mac's own address.
  On Linux it reaches every localhost port. See [the harness
  section](../../../modules/agents/agent/docs/agents/harnesses/use/security.md#your-ai-the-harness).
- **`[network] mode = "host"` is off by default,** which limits the gap
  above: an agent without it has no network but its filtering proxy,
  which closes this. [`lnk agent harness check`](../../../modules/agents/agent/docs/agents/harnesses/use/harness/README.md#inspect-the-sandbox) reports it as `open`.
- **A harness's own ports are on a Mac's loopback**, where every user
  and program of the Mac can reach them, and a connection daemon there that
  asks for no credential would let them send as the agent.
- **Each adapter guards its harness's ports,** which limits the gap
  above: each adapter keeps its harness's daemons on a unix socket in its home
  or behind a relay that asks for a secret, where its harness lets it,
  and says on its security page what it can't; on Linux behind its
  proxy, those ports stay in the sandbox's own network.
- **A program can take an agent's port** between the start's check that
  it's free and the harness's bind, or while the harness's service
  restarts. Limited by: a harness whose adapter says `proof` [proves it
  holds its endpoint's key](../../../modules/agents/agent/docs/agents/more/security.md#agents-kept-apart) before Link's connections send it the key or a
  message;
  an older adapter's endpoint is sent them unchecked.
- **Share links come only from S3 storage** (aws, s3). Other clouds'
  files are refused, never shared with a link that doesn't expire.
- **A question asked on a terminal can be faked.** With no desktop, the
  sandboxed program shares the terminal and can write over the question.
  Limited by: the code typed back allows only the call that asked; with
  a desktop it's a dialog, and [`lnk sandbox question list`](../../../modules/security/sandbox/docs/security/sandbox/tools/questions/README.md#answer-what-a-sandbox-asks) shows a waiting
  question as it is.
- **A question answered on a chat is answered by whoever holds it.**
  An agent's sandbox asks its owner's direct chats too, and the reply
  with the question's code allows the call. Limited by: the code is in
  a store only the owner reads, so the model can't hand it on; only the
  owner's account, on a chat the question was asked on, answers it; and
  it allows that one call, once.
