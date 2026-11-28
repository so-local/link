# Sandbox Isolation

> Confine Every Program to What You Grant

The sandbox [confines each program as tightly as the operating system
allows](../../../../../docs/tooling/sandbox/security.md) while the program can still run. The package `link-sandbox-confine`
does this behind one `Backend`, and only `lnk-sandbox` links it.

## What every sandbox keeps closed

A sandbox reaches the system's programs and what its specification opens,
nothing else: deny by default. [`lnk sandbox check`](../../../../../docs/tooling/sandbox/guide.md#sandbox-guide) [tries each of these](../../../../../docs/tooling/sandbox/security.md#checking-a-sandbox).

### Cleared Environment

A sandbox's environment is cleared but for `PATH`, `HOME`, `TERM`,
`COLORTERM`, `TZ`, the locale and who you are, so no token or
`SSH_AUTH_SOCK` from your shell reaches it.

### Link's Safes

Link's settings, credentials, approvals and sandbox rules live in
`~/.lnk/safe` and `~/Link/Safe`. Both stay closed to reading and writing,
on macOS and Linux, even with `--files.read ~` or a read of their parent.
Neither an explicit grant nor an internal sandbox write opens a Safe.

#### Installed programs

Programs under `~/.lnk/exec` stay hidden unless explicitly read.
A run may read its named program tree, but never write it. Data under
`~/.lnk/data` may be granted separately. The runtime folder's sockets
stay hidden except for those handed to the run.

#### Grants in or over it

A `--files.read` or `--files.write` inside a Safe is refused, as is a write inside
`programs` or of a parent of either (`--files.write ~`). Writing a parent
would let a program move the protected folder aside and replace it.

#### Runtime Grants

Explicit runtime grants survive the machine-folder mask. Harness homes and
memory's `run/` may be written. Memory's generated handoff and storage socket
folders are read-only. Linux mounts these grants after hiding the machine
folder, and macOS allows them after its deny rule. Trusted memory records,
generation caches, service logs and writable handoffs stay closed.

#### Hidden folders

A hidden folder, such as other agents' folders, is closed too, even to
`--files.read ~` and a read of a folder holding it, but for the specification's own
folders inside it.

### No grant through a link a non-root user controls

A grant reached through a symbolic link any user but root owns is
refused, naming the real path, wherever the link sits: the grant's own
root, a folder between, or one planted inside another grant. So a
program that writes a team's shared folder or an agent's files folder
can't put a link where another sandbox's grant is and bind `~/.ssh`,
another agent's keys or a folder on `PATH`, nor can a grant be such a
link itself.

#### Root's links

Only root owns the system's own links (`/tmp` to `/private/tmp`), which
no other user can plant or swap, so those are followed. This is the one
place a raw grant is resolved for a backend, so no wall follows a
planted link, whichever caller reached it: a specification's run, or an agent's
harness wrapped behind it.

#### Grant Snapshots

A specification's file grants are captured before validation and the first-run
permission question; the checks, remembered permissions and wrapper use
that same snapshot, so a directory replaced after consent cannot
redirect it, nor can a link make a protected path resolve elsewhere.

#### Pinned at launch on Linux

Every mount source is opened again at launch, one component at a time
without following links, and held until bubblewrap mounts it. A source
replaced or renamed before launch is refused, a missing optional one is
omitted, and a read-only file in `/etc` or no name reaches is copied in.
These descriptors close before the target starts.

### What runs outside from a project

At the top of each folder a sandbox writes, a project's dot files that
run outside it are read only, unless its specification names the folder
(`--files.write_dot_files`, `[files] write_dot_files`). An agent's own folder
and a session's are theirs to write; an agent's files folder is yours,
and keeps them read only. `check` tries each.

#### Dot Files That Stay Read Only

`.git`, all of it (its settings run on any git command, its hooks on a
commit or a checkout, and a file in it can point git at settings
elsewhere), `.envrc` (direnv runs it on `cd`), `.vscode` and `.idea`
(an editor's tasks, settings and run configurations), the hooks folder
and included settings files git's settings name in the folder, and what
a hook manager reads there (husky's `.husky`, `.pre-commit-config.yaml`,
lefthook's `lefthook.yml` and its other names, `.lefthook`).

#### `.husky`, and folders below the top

`.husky` is kept whole, since the stubs in `.husky/_` run the scripts
beside them. A hooks folder or included file below the top is read only,
and each folder above it, such as `scripts` for `scripts/hooks`, can't
be renamed or removed, though what else is in it is written. Where one
of those folders isn't there, the first part is kept whole instead.

#### Missing Dot Files

A hook manager's settings are held only where they're there, and on
Linux [a missing `.envrc` isn't either](../../../../../docs/tooling/sandbox/security/gaps.md#known-gaps).
On macOS, Seatbelt refuses writing each other path, there or not, in any
case of its letters. On Linux, one is made empty for the run, [but in
Link's data folders](#in-links-data-folders).

#### Read-Only Mounts on Linux

Each is mounted read only; one that isn't there is made empty for the
run (a folder, or a file for included settings) and removed when it
ends, and a link in its place refuses the sandbox. A mount can't be
renamed, removed or linked to, and a folder above one below the top is
bound over itself, so `.git`, `.husky` or `scripts` can't be moved aside
and made again. In Link's data folders, one gone by launch is left out.

#### Placeholder Files

Placeholders are made and removed by name in their folder, opened without
following a link, so another sandbox swapping a folder for a link
meanwhile can't point them elsewhere.

#### In Link's data folders

An agent's sandbox lasts as long as the agent, so a placeholder in its
Home, team folder or chats would stay in sight. No person's project is
in `~/Link/Agents` or `~/Link/Teams`, so on Linux nothing is made there.
[The file tool](../../../../../../../agents/tools/file-tool/docs/agents/tools/file/guide.md#dot-files-a-persons-tools-run) never writes the names,
and when each run ends, `lnk-sandbox` removes what a program made in
their place, by name in its folder and never through a link. A run
killed outright is swept by the next to end. One there before the run
is read only, never removed. The names and folders are one list, the
contract's `dot_files.toml`.

#### Until a run ends

A program can keep one there while its run lasts: a `run` call, a
background command, or a tool in the agent's sandbox until the agent
stops. Git or an editor used there meanwhile [runs what it wrote](../../../../../docs/tooling/sandbox/security/gaps.md#links-data-folders-on-linux).

### What sits beside `lnk-sandbox`

Every sandbox runs `lnk-sandbox` (to start under the seccomp filter,
and as ssh's ProxyCommand behind the proxy), so it reads that one file,
never the folder it's in (`~/.lnk/exec/bin`, hidden until explicitly read)
or anything else there.

### Other programs and macOS services

No sandbox reaches other programs' memory or environment, and on Linux
not their processes at all. No sandbox reaches the clipboard, the
screen, Apple Events to other apps, the Keychain, the privacy database
or core services.

### Descriptors left open (Linux)

Bubblewrap passes on every file descriptor what started it left open,
such as a CI runner's pipes to its worker, which every step inherits.
The sandbox's first program closes all but stdin, stdout and stderr
before the program starts, so the program can't write or read them. [On
macOS they stay open](../../../../../docs/tooling/sandbox/security/gaps.md#known-gaps).

### Linux Kernel

A seccomp filter refuses [the riskiest system calls](#the-sandbox-on-linux) inside every sandbox
on x86_64 and arm64; [elsewhere on Linux
there is none](../../../../../docs/tooling/sandbox/security/gaps.md#known-gaps).

### Running code the list doesn't name

Where `[commands]` lists programs, a program can't run code it wrote by
giving `exec` a file descriptor instead of a path: the seccomp filter
refuses running one (`execveat` with `AT_EMPTY_PATH`, `fexecve`), and
a filter of its own that hands its children's calls to a listener
(`seccomp` with `SECCOMP_FILTER_FLAG_NEW_LISTENER`), whose answers could
pass off a refused exec as run. Mapping memory executable for a JIT
stays open, and unrestricted executable policies keep `memfd_create`.

#### Memory files sealed

With a working listener and Linux 6.3 or newer, memory files are sealed
non-executable, so a program copied into one (`memfd_create`) can't be
run, by `execveat` or through `/proc/self/fd`. The exec watch makes
each one itself, as asked but sealed non-executable (`MFD_NOEXEC_SEAL`),
and hands it to the program as the call's answer. A memory file of huge
pages (`MFD_HUGETLB`) is refused, since Linux doesn't keep its seal: its
mode could be made executable again.

#### Without the seal

If the kernel cannot seal memory files, or the watch cannot obtain a
listener, the base filter refuses `memfd_create` with `ENOSYS` and says
why. This includes running under another listener without assuming that
it seals memory. Applications using memory files can keep doing so with
a sealing listener, and otherwise need a fallback.

### Root's capabilities (Linux)

Run by root, the sandbox drops all capabilities, in a user namespace of
its own, so its program can't override a file's permissions, reach into
another process, or open the exec watch's descriptors. The sandbox sets
`TAR_OPTIONS=--no-same-owner`, so `tar` keeps unpacked files as root's
instead of failing to change their owner.

#### Root's User Stays

The sandbox reads and writes root's files among its grants. A service
that trusts its caller's user, such as a unix socket only root may use,
takes it for root wherever the sandbox reaches it (with `[network] mode
= "host"`, the machine's abstract sockets).

## The sandbox on macOS

A Seatbelt profile, run by `/usr/bin/sandbox-exec`, denies everything,
then allows:

- Reading the system's programs and libraries (`/usr/bin`, `/usr/lib`,
  `/System`, ...), selected public system settings from `/private/etc`,
  and Homebrew's programs, libraries and certificates. Not Homebrew's
  `var/`, with local databases and logs, nor its servers' settings in
  `etc/`.
- The few devices programs use (`/dev/null`, `/dev/urandom`,
  `/dev/tty`, ...), not your other terminals.
- Reading and writing the folders its specification names.
- `stat()` on any folder: it can tell a folder exists, not what files
  are in it. Never `stat()` on files outside its folders, so it can't
  tell whether other files exist.
- Unix sockets only in its folders.
- [Listening on localhost only](../../../../../docs/tooling/sandbox/security.md#what-can-cross-the-sandbox), on the ports its specification names, and
  connecting only to those and its proxy's; with `[network] mode =
  "host"`, any address.

### Nothing else

No other localhost server (databases, dev servers, other apps' APIs),
no Keychain service, no LaunchServices, no other network. Named POSIX
shared memory and semaphores are closed: their global names are outside
the sandbox's file grants.

### Public system settings

Public system settings use the same allowlist as Linux; `/private/etc` as a whole, SSH
host keys, `master.passwd`, and private certificate keys are closed
unless the specification explicitly grants a file.

### Loopback Proxy Port

A loopback proxy binds its port before starting its program. A
dynamically assigned port is handed to that program's proxy and route
variables only once the listener is owned. Its generated Seatbelt
profile is private and unique to the run, so concurrent starts cannot
substitute one another's proxy ports.

### Launch files

Deferred launch files must be private regular files owned by the
caller, with no symlink at the file and no ancestor another untrusted
user can replace. Each initial profile and each profile prepared after
binding is unique, so concurrent wraps with the same sandbox name stay
independent.

### Process Group

The proxy gives its program a process group of its own, forwards stop
signals to that group, and kills remaining group members when the
direct program ends, before releasing its listener; a terminal run gets
the terminal and returns it.

#### Processes That Leave the Group

A process that leaves that group, or outlives a proxy killed without
cleanup, keeps Seatbelt's rules, and a later run's proxy at the same
port takes no `CONNECT` or `http://` request from it: each wrap's
secret, in `HTTPS_PROXY` alone, is required of every one.

### Rules File

Each sandbox's rules are a file in Link's settings, under
`~/.lnk/safe/sandboxes/`, so a sandbox's name is letters, digits and dashes
only.

## The sandbox on Linux

bubblewrap builds its root from the system's folders only (`/usr`,
`/bin`, `/sbin`, `/lib`, `/lib32`, `/lib64`, `/libx32`, `/opt`, `/nix`,
`/snap`, `/sys` and `/var/lib/ca-certificates`), read-only: no other
disks, `/srv`, the rest of `/var` or other homes. Of `/sys`, what only
root reads, or acts by reading, is an empty folder inside, so a sandbox
run as root reads none of it: the kernel's own (`/sys/kernel`), the
firmware's tables (`/sys/firmware`) and zram's control
(`/sys/class/zram-control`, whose `hot_add` makes a device when read).

### Other programs' sockets

A socket connects whatever its mount, so the folders daemons keep
sockets in are empty inside: multi-user Nix's
(`/nix/var/nix/daemon-socket`), which builds anything, and, where a
sandbox reads the home folder, Docker Desktop's and the like, each
socket in `~/.ssh` and `~/.gnupg` [an empty file](../../../../../docs/tooling/sandbox/security/gaps.md#on-linux-a-socket-in-a-folder-it-reads).

### `/etc` in the Sandbox

The sandbox shows only what programs read to load, find users and names, check
certificates, tell the time and start a shell, read-only, a link as the
same link. `/etc` is a folder of the sandbox's own, made read-only once
all of it is there: nothing inside writes it, root included. A program
that needs another file of `/etc` is given it by a specification's `read`.

#### Names `/etc` Shows

`alternatives`, `bash.bashrc`, `bashrc`, `ca-certificates`,
`ca-certificates.conf`, `crypto-policies`, `debian_version`, `fonts`,
`gai.conf`, `gnutls`, `group`, `host.conf`, `hosts`, `inputrc`,
`java-*`, `ld.so.cache`, `ld.so.conf`, `ld.so.conf.d`, `locale.alias`,
`locale.conf`, `localtime`, `lsb-release`, `magic`, `magic.mime`,
`mime.types`, `nanorc`, `networks`, `nsswitch.conf`, `os-release`,
`passwd`, `perl`, `pki`, `profile`, `profiles`, `protocols`, `python*`,
`redhat-release`, `resolv.conf`, `rpc`, `services`, `shells`.

#### More Names `/etc` Shows

`ssh/ssh_config`, `ssh/ssh_config.d`, `ssh/ssh_known_hosts`, `ssl`,
`static`, `system-release`, `terminfo`, `timezone`, `vim`, `vimrc`,
`xdg` and `xml`. Anything named `private` in them, at any depth and a
link so named too (`ssl/private`, `pki/tls/private`), isn't there, nor
any file whose name ends in `key.pem`, a key's.

#### Names `/etc` Hides

Not `shadow`, `gshadow`, SSH's host keys, `/etc/link`, WireGuard's or
NetworkManager's settings, which a sandbox run by root could read
whatever their modes; not what's put there by hand (`environment`,
`profile.d`, `pip.conf`, `npmrc`, `fstab`), nor what can hold a token
(`gitconfig`, `wgetrc`, `nix`); not `machine-id`.

### Home, `/run`, and `/tmp`

A sandbox gets an empty home folder holding only its folders, an empty `/run`
(no session bus), and its own PID, IPC and UTS namespaces. Every folder
any user writes, `/tmp`, `/var/tmp` and `/dev/shm`, is its own and
empty: it sees nothing another program left there, and leaves nothing
for one. (On macOS `/var/tmp` is closed, and of `/tmp` only the
sandbox's own names are open.)

### Ports and hidden folders

A sandbox reaches none of this machine's loopback ports but those its specification's
`[localhost]` names. A hidden folder is mounted over in order: the
folders holding it, then the hiding, then the specification's own folders inside
it. So Link's settings stay closed under a `--files.read` of `~/.config` too.

### First Program

`lnk-sandbox confine` closes every descriptor past stderr that
it was handed, installs a seccomp filter (x86_64 and arm64), and, where
the kernel has Landlock from its second ABI, holds the file grants again
inside the mounts, a second wall refusing any non-granted path even were
a mount wide, then becomes the program, never when that wall fails.
Every process runs under it, for good. Landlock's first ABI gets no
file wall: its layers refuse a rename into another folder, as git does.

### Refused System Calls

The filter refuses the system calls a sandboxed program doesn't need and kernel
bugs are most often found in, as container runtimes do. Each refused
call fails with an error; nothing is killed.

- io_uring (`ENOSYS`, so libuv and others fall back)
- BPF, userfaultfd, perf events
- the key store (`keyctl`, `add_key`)
- `ptrace` and reading other processes' memory, so `gdb` and `strace`
  don't work
- mounting and the new mount API
- new namespaces: `unshare`, `setns`, `clone` with a namespace flag;
  `clone3` gets `ENOSYS` so libc falls back to `clone`
- files by handle, loading kernel code, the clock, rebooting, swap
- `iopl`, `ioperm` and `modify_ldt` (x86_64)
- `TIOCSTI` and `TIOCLINUX`, which type into a terminal
- a vsock or Bluetooth socket (`EAFNOSUPPORT`): no network namespace
  holds either, so each would reach the hypervisor or the machine's
  radios whatever network the sandbox has
- every call of another architecture (i386, x32)
- a few more no program here needs, such as `acct`, `quotactl`,
  `syslog` and `process_vm_writev`; and, where `[commands]` lists
  programs, [running a file descriptor](#running-code-the-list-doesnt-name)

### User namespaces

Where bubblewrap is 0.8 or newer, it also refuses user namespaces
itself (`--disable-userns`). `lnk sandbox check` tries the calls a
program can make unprivileged, where `python3` is there to make them: a
user namespace, io_uring, userfaultfd, `keyctl` and `ptrace`.

## Ubuntu's rule for user namespaces

Ubuntu 23.10 and newer let only programs AppArmor names make the user
namespaces bubblewrap needs
(`kernel.apparmor_restrict_unprivileged_userns`).

### Make the Sandbox Ready

When that stops the sandbox, `lnk sandbox check --ready` offers, in a terminal,
to add `/etc/apparmor.d/lnk-bwrap` with sudo. [`lnk agent start`](../../../../../../../agents/agent/docs/agents/README.md#start-your-agent), `lnk
agent harness check`, and `lnk sandbox run`, `check` and `learn` run it
first; an SSH session (`enter`) checks it, and refuses without asking.
Run as root, bubblewrap's try succeeds whatever the rule says, since root
keeps its capabilities, yet the sandboxes it readies run as other users.
So where the rule is on and the profile isn't there, `check --ready` adds it
then too, as a server's boot does with `--yes` for its agents.

### AppArmor Profile

The profile is for bubblewrap alone: unconfined as before, with the right to make
user namespaces added. Every local user's bubblewrap gains that right,
as Ubuntu gives Flatpak's. The kernel attack surface the rule shields
from programs in general stays shielded.

### Declined Profile

Declined, or with no terminal, `lnk` prints the two commands, and
nothing runs unsandboxed on its own.

## Naming a refused program

Where `[commands]` lists programs, and in `lnk sandbox learn`, the wall
says which file the list refuses, or records it for `learn`. Naming is
advice: only Landlock, or Seatbelt, decides what runs.

### On Linux

The sandbox's first program (`lnk-sandbox`) stays as the parent of what
it runs, and a seccomp filter hands it each `exec` before it happens. It
reads the path from the calling program's memory, says which file the
list refuses (or, for `learn`, records it), and lets the call go on
unchanged. `learn` runs every program, under the specification's other grants.

#### After the program's own error

The call goes on once its path is read, and the refusal is named after,
so it may follow the program's own error. Four threads take the calls,
each one whole, so programs starting at once don't wait in turn; what
`learn` lists of them may come in either order.

#### What it can't name

A program can show it one path and run another, which misleads only the
message, and one that keeps its memory from being read (non-dumpable, as
`ssh` makes itself) isn't named.

#### Not dumpable

The watch makes itself not dumpable, so no program in the sandbox opens
its descriptors (the seccomp listener, `learn`'s pipe) through
`/proc/<pid>/fd` or `pidfd_getfd`, run by root too, since the sandbox
has none of root's capabilities.

### On macOS

Seatbelt reports each program it refuses in the system log, and
`lnk-sandbox`, outside the sandbox as the parent of `sandbox-exec`,
reads them with `log stream` while it runs, taking only Seatbelt's own
lines (their sender), never one another program wrote in its words.
`log stream` takes an admin user; for another, nothing is named.

#### Whose report

A report names a process and its id, not the sandbox, so it's taken for
this run's when the process is in the run's tree or already gone: a
program another sandbox refuses at the same moment may be named too.

#### How many

The reader keeps 10,000 reports at most, and once the run has started only
those taken for its own, so other sandboxes' reports never take their
room. A run that ends well with no refusal reported stops reading at
once, so a refusal reported after that isn't named.

#### For `learn`

`learn` gives its profile `(allow process-exec (with report))`, so every
program it runs is reported.

## Confinement Decisions

Confinement uses the supported operating-system mechanisms, and some
boundaries remain the caller's responsibility. [Decisions](decisions.md)
explains why.

<!-- docs-tree:start -->

## Docs

- [Decisions](decisions.md): Why the sandbox's wall, `link-sandbox-confine`, works the way it does

<!-- docs-tree:end -->
