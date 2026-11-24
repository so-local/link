# Decisions

Why `lnk` is installed, split and updated the way it is. The choices
inside `link-core`, the package that installs modules, are in its own
[Decisions](../../../core/framework/docs/more/modules/installation/decisions.md).

## Why Is the Command `lnk` and Not `link`?

The command line is "Link", but `/bin/link` already exists on macOS and
Linux, and package names are hard to change, so the command, its
modules (`lnk-<name>`) and most environment variables (`LNK_*`) say
`lnk`.

## Why Do the Core and Every Module Share One Version?

The core and every first-party module are built and signed in the same
release, one archive per program (`lnk-core-<target>`,
`lnk-<module>-<target>`). So [`lnk mod add`](guide.md#add-or-remove-a-module) always matches the core,
and [`lnk upgrade`](guide.md#upgrade) updates them together. A `lnk` built from source
never installs a release's modules, which may be another's code under
its version. It records the checkout it was built from,
and `lnk mod add` builds its modules there, as [`lnk up --source`](../developing/setup/install.md#cargo-from-source)
does. [Where modules go](../developing/setup/install.md#where-modules-go)
says where a source build puts them, and why.

## Modules from Outside Link

How a module Link doesn't build is trusted and checked before installation.

### How Is a Module from Outside Link Trusted?

By its own key, asked about once and pinned after. A harness Link
doesn't come with is released and signed by its own repository, as anyone's
adapter would be: Link's key signs only what Link's release builds (its
modules, and its harness adapters), so a
fix to an outside adapter ships when its repository releases, without
Link vouching for code it didn't build.

#### Pinned, Never Listed

`lnk` keeps no list of trusted outside keys. The first `add` shows the key and asks, and every upgrade
must be signed by the same key, so a repository taken over later can't
push a release to the machines that added it.

### What Must a Module from Outside Link Describe?

Its program must answer the generic component protocol through
`--link-describe`, using its own ID and `lnk-<id>` executable name.
A standalone program and an ID belonging to Link are refused. A
harness adapter also implements the harness contract used by the Agent
program, while other module kinds implement their own subject's interface.

## What Link Keeps, and Where

Where Link keeps what's yours and what's this machine's, and what an
uninstall takes.

### Why Two Folders, Not the XDG Ones, and Why Ask Where One Went?

An owner should be able to put their Link folder in Documents, or on
the Desktop, and have everything still work. So Link keeps what is
theirs, their agents and files, in one folder, and what is this
machine's (its programs, settings, caches, sockets) in `~/.lnk`. Every
path Link stores is relative to its folder, so either can move or be
copied.

#### Not the XDG Folders

The XDG folders spread one install over three places and leave
the owner's agents among other programs' settings. Only the machine
folder's settings name where the Link folder is, and only once it has
moved: the places of everything else are one specification that comes with Link.

#### Ask, Never Search

Link asks where a moved Link folder went rather than search. A search of the disk is slow, may open folders the owner keeps private,
and may find a copy rather than the folder they use. The folder's
marker holds an ID that this machine records the first time it sees
it, so a moved, renamed or replaced folder is told apart from a fresh
start and asked about once.

### What Does `lnk uninstall` Delete?

The programs, and only the parts you pick. Removing a tool and deleting
what it kept are different wishes: an agent's memory, your files and
encrypted buckets' passwords can't be made again, so each is its own
answer, and none is the default.

## How Does `lnk` Say a Newer Release Is Out?

[`lnk up`](install.md#pick-your-modules) and [`lnk agent show`](../../../modules/agents/agent/docs/agents/models/agent/README.md#set-its-model-and-how-hard-it-thinks), the commands a person runs to see where
things stand, show ordinary update notices at most once a day on stderr.
They never wait on the network. Ordinary notices are quiet with
`--json`, off a terminal, and with `LNK_UPDATE_NOTICE=off`. [Critical
security advisories](../vulnerabilities.md#security-update-notices) remain visible under those settings.

### What the Notice Tells

So it tells
only what the last `lnk upgrade --check` found, and starts the next in
the background a day after the last, for the next command to tell. The
version comes from the release's signed `VERSION`, as `lnk upgrade`
reads it, so nobody but Link's signer can make every `lnk` say
"upgrade".

## Why Is `lnk --help` the Groups Alone?

Every command and every module made it 122 lines, which no one reads
past the first screen. It lists the groups, each in its own help's
words, and the core's commands; [`lnk help <group>`](guide.md#get-help) lists a group's
commands, which are its module's own help, and [`lnk mod list`](guide.md#get-help) the
modules and which are installed. A word that is no group gets the one
it most likely meant: a plural or a module's name (`agents`), a
command of one group alone (`login`), or a group a typo or two away
(`agnet`).

## What a Module Needs

When `lnk` asks before it installs a module a command needs, and what
it remembers. Where a module's needs and companions are declared is in
`link-core`'s [Decisions](../../../core/framework/docs/more/modules/installation/decisions.md#what-a-module-needs).

### Why Does a Missing Module Install Without a Terminal?

Because the command typed named the feature: [`lnk agent move gcp`](../../../modules/agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud) in a
script is a yes to the box and cloud modules it needs. It installs from Link's
signed release, as `lnk mod add` would, and says what it installs, on
stderr. Only a module's own `lnk` underneath (it carries `LNK_EXE`)
installs nothing and names the command instead: a check a module makes,
such as whether you have any cloud accounts, never installs one.

### Why One Question, and Sometimes None?

In a terminal, a missing module is asked about once, naming everything
it will install with what that needs (`Install cloud, accounts and
box?`), after the command's arguments are read. `lnk <group>` whose own
module is missing asks before: only that module can read its arguments.
A command that names the
module itself, such as [`lnk agent connect slack`](../../../modules/agents/agent/docs/agents/README.md#connect-your-channels), [`lnk agent harness set hermes`](../../../modules/agents/agent/docs/agents/harnesses/use/README.md#switch-in-one-command) or [`lnk agent tool add web`](../../../modules/agents/agent/docs/agents/README.md#add-tools-and-skills), asks nothing: naming it was the yes.

### Why Doesn't the Storage Module Need the Cloud Module?

A bucket in a folder uses no cloud. So the cloud module is installed
when a bucket names a cloud account, the way a box's cloud adapter is,
not with every storage module.

### How Is a Refusal Remembered?

Where it was made: the tools an agent is offered, in that agent's
settings; anything else, on this machine, in `~/.lnk/safe/modules/offers.toml`.
Neither is asked again; only the command that adds it (`lnk agent tool
add`, `lnk mod add`) does. A line said without a terminal isn't a
refusal: a later start in a terminal still asks. Memory's embedding
model is offered once, when memory is picked, and memory says it
searches by words only without it.

### Why Is a Harness's Runtime Asked About Every Time?

It's a large download (DeepSeek Harness's is about 800 MB), so even
`lnk agent harness set deepseek`, which names it, asks first, saying what it
downloads; without a terminal it downloads only with `--yes`.
