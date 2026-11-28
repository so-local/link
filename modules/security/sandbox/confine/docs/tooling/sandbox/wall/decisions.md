# Decisions

Why [the sandbox's wall](README.md), `link-sandbox-confine`, works the way it does.

## What it closes

Why the wall closes what it closes on each OS, and how it holds a
folder's grants.

### Why can't a sandbox reach the Keychain?

No harness needed it. A sandbox that can ask `securityd` for items can
be handed a saved password, such as git's for GitHub, and send it out;
a tool that runs outside gives a program what it needs of your logins,
asking you. So the `keychain` permission went, and a stored one is read
as nothing.

### Why does Linux use a seccomp filter?

The kernel is a wall too. The filter refuses by name the calls a
sandboxed program doesn't need and container runtimes refuse too, each
with an error rather than killing the program, so it can fall back.
`lnk-sandbox` installs it as the first thing inside, rather than passing
it to bubblewrap as a file descriptor, because a harness's sandboxed
command is kept and run later by the background service, which can't
carry one. It is built by hand from `libc`, a few dozen BPF
instructions, with no new dependency.

### Why can a sandbox run without Landlock?

Capabilities follow the effective policy after OS overrides and harness
requirements. Bubblewrap and the syscall filter enforce file and network
restrictions with or without Landlock; where the kernel has it, Landlock
holds the file grants again inside the mounts — a second wall it keeps
even were a mount wide — and absent it the mounts stand alone.

#### What needs it

A finite
allowlist, or a policy granting no programs, needs Landlock and refuses
the launch without it. The sandbox never substitutes a classifier or
unrestricted execution; an allowed interpreter still runs any code, but
only within the file and network boundaries it inherited.

### Why do executable tests prove their target started?

A refused launch establishes that unsupported policy fails closed. It
does not prove that the kernel restricted a running process. Executable
enforcement tests therefore require the capability and verify startup
inside the sandbox before counting a refused executable. File and
network tests ask for unrestricted programs so their own boundaries
still run without Landlock. Full coverage requires both capability
environments. Containers share their host kernel and cannot add missing
Landlock; a separate virtual-machine backend is independent of this
capability check.

### How does hiding a folder work?

A hidden folder is closed whatever else is open, but for what the
specification names in it. An agent's sandbox hides every agent's folder and
reopens its own, so an agent made later is hidden too. On Linux the
folders holding a hidden one are mounted before it's hidden, and the
ones inside after. On macOS the deny follows `[files] read = ["~"]`, and the
specification's own folders in there are allowed again after it.

### Why does a sandbox run by root drop every capability?

Run by root, bubblewrap keeps root's capabilities inside, user
namespace or not. With them a program reads and writes any file it's
given whatever its permissions, and opens other processes' descriptors
and memory, the exec watch's included, which not being dumpable doesn't
stop. A sandbox run by root, on a box, needs none of them: it writes
root's own folders, and the proxy's DNS helper joins its network from
outside. So it drops them all (`--cap-drop ALL`) and always gets a user
namespace of its own, which the helper joins as its owner. It keeps
root's user, so files it writes stay root's.

### Checked file grants are a snapshot

A specification's file grants are resolved before they're checked and asked
for, and an opaque snapshot carries those names through remembered
permissions, description and wrapper, never resolved again. Validation
holds both the name and its current target; below another grant only
its root is resolved, the suffix literal, so a link planted after
validation redirects nothing. Linux pins the sources at launch too.

### Linux mount sources are pinned at launch

A wrapped command may be saved and run later, and a writable ancestor
replaced before bubblewrap starts. So the launch stage opens every
source one path component at a time without following links and hands
bubblewrap a held descriptor; a missing optional source is left out.
The inner confiner closes inherited descriptors before the target runs.

#### Pinned Files No Name Reaches Are Given by Their Contents

Bubblewrap mounts what a held descriptor's name reaches, and the kernel
binds no mount held from another mount namespace. A file replaced by a
rename, as `useradd` replaces `/etc/group`, has no name left, nor has
one an outer sandbox shows after the machine replaced it. So a
read-only file like that is given by its contents (`--ro-bind-data`)
with its mode. Refused instead, every sandbox an agent's harness starts
inside its own would fail after each `useradd`. A folder or writable
file like that is left out where optional and refused otherwise.

#### Read-Only Files From the Machine's `/etc` Are Always Given by Their Contents

A read-only file from the machine's `/etc` is given by its contents
even while its name reaches it. Tools replace those by renaming at any
moment, and a Link box's workers run `useradd` for every launch, so a
name checked at launch may reach another file by the time bubblewrap
resolves it. Mounted by name, about one agent start in fifteen failed
on a box running cases side by side.

#### A Mount Another Covers Is Left Out

A mount whose place a later mount of the same place covers shows
nothing, yet bubblewrap would still resolve its source, so it's left
out. The sandbox's own `/etc/hosts` covers the machine's, which bubblewrap
couldn't mount over once given by its contents.

#### Dot Files in Link's Data Folders Are Optional Mounts

Another run's sweep may remove a dot file there at any moment. One gone
by launch has nothing left to keep, so it's bound with the `-try` form.

### Named IPC on macOS stays closed

POSIX shared memory and semaphores have host-wide names, not a specification's
paths, so both stay denied: another program's memory stays outside.
## The programs it may run

Why `[commands]` limits files, and how each OS keeps a program it
doesn't list from running.

### Why does `[commands]` limit files and not code?

The kernel sees which file is run, not what an interpreter is told to
do: Seatbelt and Landlock both check the file executed. Checking a
program's arguments would need something outside watching each start,
which Linux does only with a race and macOS only with an entitlement.
So the list says which programs may start, and the docs say an
interpreter runs anything.

### Why is running a file descriptor refused, but not `memfd_create`?

`[commands]` lists which files may run, by the file `exec` is given. A
program can still run code it wrote by handing `exec` a descriptor
instead of a path: `execveat` with `AT_EMPTY_PATH` (`fexecve`), or a
`/proc/self/fd` entry of a program copied into a memory file
(`memfd_create`), which is in no folder for Landlock to judge. The
seccomp filter refuses running a descriptor, and the sandbox makes memory
files non-executable, so neither runs.

#### `memfd_create` itself

`memfd_create` itself stays open,
and mapping its memory executable stays open, because Chromium and some
JITs need them, when the kernel can seal the files and a listener can
apply that seal. If either is unavailable, the base seccomp filter
refuses `memfd_create` with `ENOSYS` instead.

#### What that keeps

This preserves the
executable restriction on older kernels and under another listener,
without assuming what that listener enforces. Unrestricted programs
keep memory files, while applications under an executable allowlist
need a memory-file fallback when sealing is unavailable.

### Why does the exec watch make the sandbox's memory files?

The kernel's own control, `vm.memfd_noexec`, can be set per PID
namespace, but only by a process with `CAP_SYS_ADMIN` there whose user
is the machine's root: a sandbox run by anyone else never can, and one
run by root no longer has the capability. There is no per-process
switch.

#### Handed to the exec watch

Refusing `memfd_create` without `MFD_NOEXEC_SEAL` in the seccomp
filter would break every program that makes one plainly, JITs among
them, and a filter can't change a call's flags. So the filter hands such
a call to the exec watch, already there where programs are listed,
which makes the file with `MFD_NOEXEC_SEAL` added and puts it in the
program as the call's answer (`SECCOMP_ADDFD_FLAG_SEND`). The program
gets a memory file it can write, seal and map executable, but never
run, and the kernel refuses making it runnable later.

#### Sealed memory and huge pages

A call that asks
for sealed memory itself, the watch's own among them, runs as it is. A
memory file of huge pages (`MFD_HUGETLB`) is refused with `EINVAL`, as
on a kernel without huge pages: hugetlbfs takes the seal but doesn't
enforce it, so `fchmod` makes the file executable again and it runs.
Programs that want huge pages fall back to plain memory.

#### Without the seal

Without the
seal or the listener, creation is refused: a warning alone would let a
memory file run around the allowlist through `/proc/self/fd`.

### Why can't the loader and `LD_PRELOAD` be closed?

Every dynamically linked program needs its loader run on its behalf, and
Landlock's execute right applies to it, so the loader must stay runnable
where programs are listed; a program can then hand the loader any file it
can read (`ld.so ./x`). The loader's injection variables are cleared when
the sandbox starts, but a program can set `LD_PRELOAD` again for a child
it runs. Both stay a stated gap, which `check` reports as open.

### Why does the sandbox name a refused program itself, not Landlock's audit log?

Landlock logs what it refuses to the kernel's audit log (Linux 6.15 and
newer), but only where auditing is on, and only a process with
`CAP_AUDIT_READ` reads it: almost never a person's own `lnk`. So the
sandbox watches each `exec` itself, through a seccomp filter that hands
it the call before it happens, and judges the file against the same
list Landlock enforces. It never answers for Landlock: every call goes
on, and Landlock alone refuses. The same watch is what `lnk sandbox
learn` records, so finding the programs a command runs needs no
privilege either.

### How does macOS name a refused program?

From Seatbelt's own reports: it writes a line to the system log for each
program it refuses (`deny(1) process-exec*`), and nothing else on a Mac
sees an `exec` without privileges a person's `lnk` doesn't have. So
`lnk-sandbox` stays outside as the parent of `sandbox-exec`, reads them
with `log stream` while the program runs, and says which file the list
refused, as on Linux.

#### For `learn`, and its limits

For `learn`, the profile allows every program
`(with report)`, so each is logged as it runs. A report names a process,
not a sandbox: one whose process is still there and not of this run is
left out, but one that's gone can't be told apart, so another
sandbox's refusal at the same moment may be named too. Naming is advice,
never a decision: Seatbelt alone decides what runs.

### Why does `lnk sandbox learn` run every program, once?

A list written by hand misses the helper a program starts only now and
then (git's `git-remote-https`, a compiler's linker), and a run that
fails on it says only "Permission denied". `learn` runs the command once
under the specification's other grants with every program open, and prints what
to add, writing nothing: the person adds the lines, so what a specification runs
is still what they wrote. It never offers a program the sandbox could
replace (in a folder it writes, or a temp folder), and marks
interpreters, which run any code.

## A project's dot files

Why the files of a project that run as you outside the sandbox are read
only at the top of a folder it writes.

### Why are a project's dot files read only?

A folder a sandbox writes is one you work in afterwards, outside it. In
a project, some files run as you without your running the project:
git's settings and hooks on any git command, `.envrc` when direnv sees
you `cd` in, an editor's tasks and settings when it opens the folder.
So at the top of every folder a sandbox writes, these are read only by
default, the least that still does the job, as a specification's missing key is.

#### Reading, and committing

Reading a repository (`git status`, `diff`, `log`) still works. A
sandbox that commits, or changes git's settings, gets them with a key in
`[files]`, `write_dot_files`, listing the folders of `write` it opens,
so each is written down, and a specification not run before shows it.

### Which dot files, and why all of `.git`?

The ones that run on what you'd do anyway, not on running the project:
`.git`, `.envrc`, `.vscode` and `.idea`, the hooks folder and included
settings files git's settings name in the folder, and what a hook
manager run from `.git/hooks` reads (pre-commit's and lefthook's
settings). `.husky` is kept whole: husky's stubs in `.husky/_` run its
scripts in `.husky`.

#### Below the top

A hooks folder or included file below the top is
read only, and the folders above it can't be renamed, since a folder
above a mount could be renamed and made again; they stay writable
otherwise, so `scripts/hooks` doesn't close all of `scripts`.

#### All of `.git`

All of `.git`, not only its settings and
hooks: a file git reads there, `commondir`, points it at settings
anywhere, and closing its settings alone would leave that open. That is
also why a commit needs the key.

#### What stays writable

A project's own build files (a
Makefile, `package.json`'s scripts) stay writable: they run when you
build the project, which runs whatever the sandbox wrote anyway. A dev
container's settings stay too: their commands run when you choose to
open the project in a container.

### Why only at the top of a folder it writes?

That's the folder you gave it and will work in. A repository it clones
or makes in a folder inside is its work, like any file it writes, and
cloning one is common work. On Linux nothing short of a
mount closes a path, a mount needs something there, and a sandbox makes
new folders as it runs, so a rule for any depth couldn't hold there; the
same rule holds on macOS, so a specification does the same on both.

### Why does an agent write its own folder's dot files, but not its files folder's?

An agent's own folder holds only what its harness keeps, out of every
other sandbox's reach, and may hold a repository (a workspace it
commits). Nothing of yours runs from it. Its files folder
is yours: you open it, and may run git in it. So an agent commits in a
repository it makes in a folder inside, not in one at the top of your
files folder. A session's folder is theirs, as an agent's own is.

### How does Linux keep a missing dot file from being made?

A mount covers only what's there, and a write grant otherwise lets the
program make a `.git` that wasn't. So the `lnk-sandbox` that starts
bubblewrap puts an empty folder there for the run, and removes it after,
unless a run still alive holds it: removing one another sandbox has
mounted over undoes that mount. Each is listed in Link's settings with
the runs holding it, and goes only while it's the empty one Link made. A
link is refused: the program could point it elsewhere.

### Why are Link's data folders swept rather than held?

A placeholder lives as long as its sandbox, and an agent's lives as long
as the agent. So its Home, its team's folder and its chats would show
empty `.git`, `.vscode` and `.idea` folders for good, to you and to the
program working there, which takes them for its own. `~/Link/Agents` and `~/Link/Teams`
hold no person's project, so no tool of yours opens them as one, and a
name made there can be removed rather than covered. The file tool refuses
the names, and each run's end removes what a program made.

#### Why not Landlock?

Landlock, Linux's unprivileged access control, only allows. A rule grants
rights on a folder and everything beneath it, with no rule that denies
and no rule by name. Making files at the top of a Home needs the right to
make any name there, `.git` among them. Refusing a name without anything
on disk would take a supervisor reading each path from the program's
memory, which another of its threads can change after the check.

#### What a sweep leaves open

A program can make a name and leave it there until its run ends: a `run`
tool call's end, or a background command's. While it's there, git run in
that folder runs what it wrote. A sweep removes what's there, so a
`.git` you make by hand in an agent's Home while one of its runs lasts
goes too.

### Why isn't a missing `.envrc` held on Linux?

direnv runs an `.envrc` only once you've allowed it with `direnv
allow`, and again only after you allow a changed one, showing it each
time: a new one runs nothing by itself. A placeholder there would be a
folder where direnv and you expect a file, so a missing `.envrc` is left
alone on Linux; one that's there is read only, and on macOS Seatbelt
refuses making one anyway. In Link's data folders a new one is swept
like the others.

## What a start costs

A start's checks of the machine are kept for the boot, passes only,
opening nothing: bubblewrap, the filter and Landlock still refuse what
they can't do. bubblewrap keeps its pid 1: as pid 1, a program gets no
signal it has no handler for from inside its namespace, Ctrl-C too, and
must reap orphans.

### The exec watch and the placeholder lock

On 4 CPUs the exec watch adds about 0.2 ms to an exec
and 6 to 13% to throughput at 8 to 16 concurrent loops (four receivers;
eight gain nothing), and the placeholder lock about 1 ms to a start, a
median 25 µs wait among 16 at once: narrowing either adds states to the
code pinning mounts and removing placeholders, for no visible gain.
