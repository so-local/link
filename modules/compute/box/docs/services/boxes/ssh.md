# Reach a Box

`lnk box ssh` opens a shell on a box, or runs one command there, over
OpenSSH with Local Link's own key and a certificate that lasts an hour.
`lnk box run`, `lnk box copy` and `lnk box forward` reach the box the
same way, so this page explains the connection once for all four.

```sh
lnk box ssh aws-1
```

## Open a Shell on a Box

`lnk box ssh` logs you in to the box as its `ubuntu` user, in a login
shell that finds `lnk`, since the box's setup adds `~/.lnk/exec/bin` to
the `PATH` in `~/.profile`. Name the box as `lnk box list` shows it.
With one box, its name can be left out. With several and no name, it
refuses and lists their names, so you can pick one.

### Run One Command

Words after `--` run as one command instead of a shell, and its exit
status is the command's. Several words are a command and its arguments,
each quoted so it arrives whole. One word is a shell line of its own, so
pipes and `&&` in it run on the box. A terminal is opened when there is
no command, or when your input is a terminal, so an interactive program
works, and a piped command gets your bytes untouched.

```sh
lnk box ssh aws-1 -- df -h /
lnk box ssh aws-1 -- 'journalctl -u ssh | tail -n 20'
```

### What SSH Needs

Your computer needs OpenSSH's `ssh` and `ssh-keygen`, found on your
`PATH` in absolute folders, and the box's cloud account connected with
[`lnk cloud connect`](../../../../../cloud/cloud/docs/services/cloud/guide.md#connect-a-cloud-account).
The account is used the first time this computer reaches a box, to read
its host keys and let this computer's key in. The box must be running,
with an address, and its port 22 must be reachable from your network,
or the cloud's tunnel must be chosen instead. On the box, Ubuntu's own
`sshd` does the rest: the box's first boot told it to trust this
computer's certificates.

## How SSH Connects

Each connection follows the same steps, whichever command makes it.
Local Link finds the box in `~/.lnk/safe/boxes/boxes.toml`, refuses a
stopped box or one with no address yet, and gets in for the first time
when the box was made elsewhere or before certificates. It then signs a
fresh certificate when the one it has is over half an hour old, and runs
your `ssh` as `ubuntu@<address>` with these credentials and checks.

### Credentials for Each Connection

`~/.lnk/safe/boxes/id_ed25519` is Local Link's own SSH key, which
`lnk box key show` prints, as [Box Key](user/key/README.md) explains. It opens a box
only with a certificate, and `ssh` is given it with `IdentitiesOnly=yes`,
so no other key of yours is offered, and waits at most 10 seconds to
connect.

### Certificates

Each computer has an SSH certificate authority of its own, and a box
trusts the authorities of the computers that reach it: a copied key
reaches nothing once its certificates expire, at most an hour later.

#### Who Holds What

The authority's secret stays on the computer that made it. It's an
ed25519 secret made by `ssh-keygen`, kept in the Keychain on a Mac
(`box/ssh-ca`), else in `~/.lnk/safe/boxes/ssh-ca`, mode 600 in a 700 folder,
and its public half in `ssh-ca.pub`. It never reaches a box or a cloud.
This computer's key, `~/.lnk/safe/boxes/id_ed25519`, ed25519 with no
passphrase, is what a certificate certifies.

#### What a Box Trusts

A box made here trusts this computer's authority from its first boot.
Its cloud-init writes the authority's public half to
`/etc/ssh/lnk/user-cas.pub` and the box's principal to
`/etc/ssh/lnk/principals/ubuntu`, both root's. A drop-in,
`/etc/ssh/sshd_config.d/10-lnk.conf`, points sshd's `TrustedUserCAKeys`
and `AuthorizedPrincipalsFile` at them. The first boot also drops
Link's plain keys from `authorized_keys`, so a box made from an older
image takes none.

#### One Certificate

A certificate names one principal, `lnk-box-<installation id>`, so it
reaches one box. It lasts an hour, from five minutes before it's
signed, and allows a terminal and forwarded ports, never your SSH keys or
X11. Before a connection, `lnk box` signs a new one with `ssh-keygen -s`
when the box's in `~/.lnk/safe/boxes/certs/` is over half an hour old. The
secret is written to a 700 folder of `~/.lnk/safe/boxes` while `ssh-keygen`
reads it, then removed.

#### Boxes Made Before Certificates

A box made before certificates is given them the next time Link reaches
it. Link gets in with this computer's key and adds the authority and
the principal as a first boot would. Once a certificate alone gets in,
Link's plain keys go, through the adapter's `remove-keys` and from
`authorized_keys`, anyone else's kept. A box with no installation ID yet
keeps the key until `lnk box list` reads its ID.

### Host Keys

A box's SSH host keys are pinned before the first connection, from what
the cloud says, and checked on every one after.

- A new box's SSH host keys are read from its console output, the block
  cloud-init prints at the first boot, through the cloud's API as your
  account.
- They're pinned in Link's own `known_hosts` before the first SSH
  connection, and kept in `boxes.toml`. SSH then refuses any other key:
  once Link's `known_hosts` holds a box's keys, every connection checks
  them with `StrictHostKeyChecking=yes`.
- They're kept under the box's own name, `lnk-<cloud>-<its id>` (ssh's
  `HostKeyAlias`), never its address, so a box at a new address,
  restarted here, from the cloud's console or by another computer, is
  checked against the same keys.
- When the console shows none within 5 minutes, or the adapter can't
  read it, the key is accepted on first contact and pinned after, under
  the box's name too, and Link says so. That trusts the network between
  you and the cloud once.
- The first contact (`accept-new`) writes the
  key to a file of its own, `known_hosts.first-<name>`, never to Link's
  `known_hosts`, which only Link writes, under its lock: the next
  connection reads the key back, pins it there and checks it strictly
  from then on.
- An address is one box's: a box given an address another box of yours
  was last seen at (stopped since) takes it, so the keys, certificate and
  tunnel Link finds by the address are that box's alone.
- A removed box's keys are forgotten, since its address can come back as
  another machine's.
- The computer that pinned them keeps them in the cloud, since the
  console's first-boot output doesn't last: metadata `lnk-host-keys` on
  Google Cloud; on AWS, the ed25519 key in a `lnk-host-key` tag, which
  holds 256 characters. Another computer pins those before its first
  connection, and reads the console only when the cloud keeps none.
- Anyone who controls the cloud account can change the host keys kept
  there.

### First Connection From Another Computer

A box made on another of your computers is reached the first time by
pinning its [host keys](#host-keys) from the cloud. Then this computer's key
is let in through the account for a moment, while the box is made to
trust this computer's authority too, and then the key goes.

#### On Google Cloud

Merged into the instance's `ssh-keys` metadata, never replacing the
others, until `remove-keys` takes it out again. A box takes only its own keys: it's made with
`block-project-ssh-keys=TRUE`, so the project's SSH keys, which anyone
who can edit the project's metadata adds, don't let anyone in, and with
OS Login off. A box made before this takes them until it's made again.

#### On AWS

Pushed with EC2 Instance Connect, which lets it in for 60 seconds,
during which `lnk box` appends it to `authorized_keys` over SSH, to drop
it once a certificate gets in.

#### Each Computer's Key

Each computer's key has the comment `lnk <installation id> <machine name>`,
and its authority `lnk-ca <installation id> <machine name>`, so a lost
computer's can be found in `/etc/ssh/lnk/user-cas.pub` and removed.
Whoever controls the account could add a key this way too: the account
is trusted, as it could replace the machine anyway.

### One Connection per Box

The SSH calls a command makes to one box share one connection, which
any program running as you can use while it's open.

- The `lnk box run` calls a command makes to one box share one SSH
  connection, which the first opens with the key and host keys above.
  It closes a minute after the last one ends, and when the box is
  removed or a new box gets its address.
- Its socket is in `~/.lnk/safe/boxes/mux`, a 700 folder. While it's
  open, any program running as you can run commands on the box through
  it without the key, as it could with the key and the authority's
  secret, which it can read too.
- When that folder's path is too long for a socket (over 42 characters),
  each call connects on its own.

### Through the Cloud's Tunnel

With `LNK_BOX_SSH=tunnel`, where port 22 can't be reached, every
connection to a box goes through its cloud's own tunnel over HTTPS
instead: ssh's `ProxyCommand` is the box's adapter's `tunnel`, which
runs AWS Systems Manager's `AWS-StartSSHSession` or Google Cloud's
`start-iap-tunnel` as the box's account. An AWS box made so opens no
port: its security group, `lnk-box-tunnel`, made and removed like
`lnk-box`, lets nothing in.

#### Still SSH

It's still SSH, end to end, with Link's certificate, and the box's host key
checked as it was pinned: the tunnel carries the bytes and sees none of
them in the clear.

#### Who Can Open a Tunnel

Anyone the cloud lets open a session or an IAP tunnel to the box can
open one: on AWS, whoever is allowed `ssm:StartSession` on the
instance; on Google Cloud, whoever has
`roles/iap.tunnelResourceAccessor` in its project. They still meet the
box's SSH, which takes only Link's certificates.

#### Systems Manager's Other Documents

On AWS, the box's Systems Manager service also takes other documents, a
shell as `ssm-user` among them, which needs no SSH key: give the users
that open tunnels `ssm:StartSession` on `AWS-StartSSHSession` alone.

#### `ProxyCommand` for the Tunnel

It names the adapter, the account, the box's place and its id, each
refused unless it's letters, digits, dots, dashes and underscores, none
first a dash, and each quoted for the shell ssh runs it with. The
account's credentials reach the cloud's CLI in its environment, never
on a command line; AWS's own short-lived session token is on
`session-manager-plugin`'s, as `aws ssm start-session` runs it.

### Your Own SSH Config

Every connection runs your own `ssh` with none of your SSH settings
but their `ProxyJump`, which still works: the jump host is reached by
an `ssh` that reads them. What would hand the box something of this
computer, such as your keys, X11 or a forwarded port, is turned off.
[Security](security.md#your-own-ssh-settings) lists each option and why.

## Shells on a Box Here

A box here has no SSH server, since it is a folder of this computer.
`lnk box ssh` starts your `$SHELL`, or `/bin/sh` when that isn't an
absolute path, in the box's home, under the box's sandbox or directly
with `--no-sandbox`, as [Boxes Here](guide.md#boxes-here) describes. A
command runs as `sh -c` with the same quoting as on a cloud box.

## How SSH Fails

`lnk box ssh` refuses before it connects when it can't tell which box
you mean, when the box is stopped, and when it has no address yet, each
with the command that fixes it. A missing `ssh` or `ssh-keygen` is
named before anything else runs. Once `ssh` runs, its exit status is
the command's, so a failed connection exits with `ssh`'s own status 255
and its message. [Troubleshooting](troubleshooting.md#reaching-a-box)
has each message and its fix.

### Changed Host Keys

A box whose host key no longer matches the pinned one is refused by
`ssh` itself, with its warning that the remote host identification has
changed. Local Link never accepts the new key on its own, since that is
what a machine pretending to be the box would show. A box that was
deleted and made again under the same name has new keys, and its old
ones are forgotten when it's deleted.

### Boxes That Don't Answer

After `lnk box create` and `lnk box start`, Local Link waits up to 5
minutes for the box to take SSH, trying every 5 seconds, then stops with
the command that asks its cloud for its state. Within a connection,
`ssh` sends a keep-alive every 30 seconds, so a dropped network ends the
session instead of hanging it.

## Run `lnk` on a Box

`lnk box run` runs `lnk` on a box over the same connection, with the
words after `--` as its arguments. Its input and output pass through
untouched, and its exit status is the remote `lnk`'s, so another
program can pipe data through it. The `lnk` there follows the releases this
computer's does, as [What's on a Box](lifecycle/on-a-box.md#lnk_releases_url) says.

```sh
lnk box run aws-1 -- module list
```

## Copy Files to a Box

`lnk box copy` copies files from this computer into a folder on a box,
each keeping its name and permissions, and makes the folder when it's
missing. The folder is read from the box's home folder, and its name
reaches the box's shell quoted, so nothing in it runs. It writes over
the box's SSH, never through the relay, up to 64 files in one session.
A folder is not copied whole, so archive it with `tar` and copy the
archive.

```sh
lnk box copy aws-1 deps.toml install.sh setup
```

### Refused Links


It refuses a link rather than follow it, the file itself or any folder
on the way to it in your home, so a link planted where you copy from
can't send what it points to (an SSH key) to the box. It opens each
folder from the one before without following a link, so a folder
swapped for a link after the first check is refused too.

### Hard Links

A file with another name (a hard link), which could be one anywhere on
the disk, is refused as well: copy it to a new file first.

### How Files Are Written

Each file is written beside its place, its size checked, and moved in
once whole, with its permissions, in a folder made only for `ubuntu`
(`umask 077`) when it's missing. A file whose size changed after it was
opened is refused.

## Forward Home to a Box

A box can send its traffic out through this computer, so sites see your
home address. `lnk box forward` gives the box a SOCKS proxy that runs on
this computer's loopback, on a Unix socket in the box's home folder or
on a port of the box's loopback, until the SSH connection drops. SOCKS
is a protocol a program speaks to a proxy, asking it to connect
somewhere on its behalf.

```sh
lnk box forward aws-1 --socket proxy.sock --proxy-port 1080
lnk box forward aws-1 --port 40780 --proxy-port 1080
```

### Proxy Requirements

`--proxy-port` names a SOCKS proxy you run on this computer, one that
refuses this computer and its networks, so the box can't reach them
through it. `ssh`'s own SOCKS proxy, which refuses nothing, is never
used. The socket is the safer choice: only the box's user reaches it,
and no SSH setting opens it to the network. A box with
[users of its own](user/README.md) refuses `--port`, since every one of
them reaches a port on its loopback: only the socket is forwarded
there. A box here refuses
forwarding, since it is this computer already. How the socket's folder
is prepared is in the [box contract](../../../contract/docs/services/boxes/more/contract.md#a-socket-forwarded-over-ssh).
