# Features

Every feature of the sandbox, one row each, grouped by what it opens or
closes. Each row says what the feature lets through and what it
refuses, and a test of a row shows both. A row's name, in the last
column, stays the feature's for life, so a test can name the row it
proves. **Where** is `both` for Linux and macOS alike. How each works
and what it guarantees is in [Security](../security.md); every key of a
specification, in the [specification](../../../config/specs/sandbox.md). Try a shell that can write only the current folder.

```sh
lnk sandbox run --commands.toolchains shell --files.write . -- sh
```

## Files

What a sandbox reads and writes of your files. Agents' folders are
hidden to every sandbox, Link's settings, its Safes (`~/.lnk/safe`,
`~/Link/Safe` and the system's), are closed read or written but for a
harness's own home there, and a grant reached through
a link is named by its real path.
A project's dot files are `.git`, `.envrc`, `.vscode`, `.idea` and hook
managers' files. A short `/etc` keeps what programs read to load, find
users and names, and check certificates, refusing `shadow`, SSH's host
keys, anything named `private`, and writing any of it.

| Feature | Lets through | Refuses | Key, flag | Where | Row |
|---|---|---|---|---|---|
| Closed by default | The system's programs and libraries, read only | Every folder of yours no grant names, your home among them | none | both | `files.closed` |
| Folder it writes | Reading and writing in the folder, and below it | Writing beside it, in another folder of your home | `[files] write`, `--files.write` | both | `files.write` |
| Folder it reads | Reading in the folder | Writing in it | `[files] read`, `--files.read` | both | `files.read` |
| Your whole home, read | Reading every folder of your home | Link's settings, agents' folders, writing | `read = ["~"]`, `--files.read '~'` | both | `files.home` |
| Folder hidden | The `write` and `read` folders inside it | Everything else in it, even under `--files.read '~'` | `[files] hide`, `--files.hide` | both | `files.hide` |
| Agents' folders hidden | A folder in them that flags name, as its harness has | `~/Link/Agents`, `~/Link/Archive`, named in a specification | none | both | `files.agents` |
| Link's settings closed | Link's programs by their file; a harness's own home | The Safes; a grant in one, a write holding one | none | both | `files.settings` |
| Project's dot files read only | Reading them at the top of a folder it writes | Writing, renaming or removing them | none | both | `files.dot` |
| Project's dot files written | Writing them in a folder of `write` it names | Naming a folder that isn't in `write` | `[files] write_dot_files`, `--files.write_dot_files` | both | `files.dot-write` |
| No grant through a link | A link root owns, such as the system's own | A grant through a link another user owns | none | both | `files.links` |
| Temporary folders of its own | `/tmp`, `/var/tmp` and `/dev/shm`, empty and its own | Seeing what another program left there | none | Linux | `files.tmp` |
| Short `/etc` | What programs read to load, name users and check TLS | `shadow`, SSH's host keys, `private`, any write | none | Linux | `files.etc` |
| Descriptors closed | stdin, stdout and stderr | Every other descriptor what started the sandbox left open | none | Linux | `files.descriptors` |

## Programs

Which programs it may run: with no specification, any the files grant; with
one, those its `[commands]` lists, and never one it could write.

| Feature | Lets through | Refuses | Key, flag | Where | Row |
|---|---|---|---|---|---|
| Toolchains | A toolchain's programs and its builds' | Every program it doesn't name | `[commands] toolchains`, `--commands.toolchains` | both | `programs.toolchains` |
| Programs it may run | The programs, paths and folders it lists | Every other, with "Permission denied", named by the sandbox | `[commands] allow`, `--commands.allow` | both | `programs.allow` |
| Specification without `[commands]` | Nothing | Every program, the one `run` starts too, before it starts | none | both | `programs.none` |
| Any program | Every program, `"*"` alone in `allow` | Nothing more: files and network still hold | `allow = ["*"]` | both | `programs.star` |
| No program it can write | A program in a folder it only reads | An entry in a folder it writes, or in `/tmp` | `[commands] allow` | both | `programs.writable` |
| No code run from a descriptor | Running a file by its path | `execveat` of a descriptor, a memory file made executable | none | Linux | `programs.descriptor` |
| Learning a command's programs | Running every program once, under the other grants | Writing anything: it prints the entries | [`lnk sandbox learn`](../../../../confine/docs/tooling/sandbox/wall/README.md#naming-a-refused-program) | both | `programs.learn` |
| Xcode's programs | Reading the developer tools | Writing them | `developer_tools`, `--commands.developer_tools` | macOS | `programs.developer-tools` |

## Network

How it reaches the network at all. A cleared environment refuses every
other variable of your shell, tokens and `SSH_AUTH_SOCK` among them;
where its traffic leaves is `direct`, a SOCKS5 proxy, or an HTTP proxy
on this machine's loopback.

| Feature | Lets through | Refuses | Key, flag | Where | Row |
|---|---|---|---|---|---|
| No network | Nothing | Every host, port and name, inside and out | none | both | `network.none` |
| Through Link's proxy | What the proxy's sections allow, at `HTTPS_PROXY` | A program that ignores `HTTPS_PROXY`: it has no network | `mode = "proxy"`, the default | both | `network.proxy` |
| Network as it is | Any host and port, unfiltered and unlogged | The sections a proxy carries, beside it, in a specification | `mode = "host"`, `--network.mode host` | both | `network.host` |
| Where its traffic leaves | `direct`, SOCKS5, or an HTTP proxy on this loopback | A Unix socket another user serves, a link in its place | `--upstream` | both | `network.upstream` |
| Cleared environment | `PATH`, `HOME`, `TERM`, the locale and who you are | Your shell's other variables, `SSH_AUTH_SOCK` too | none | both | `network.environment` |

## Internet

Public hosts, through the proxy. Never this machine or its network
refuses loopback, private, link-local and metadata addresses however
written, and a name resolving there. Named endpoints refuse an endpoint
whose host isn't allowed, and DNS answers the names its hosts allow,
and its endpoints' names at addresses of its own.

| Feature | Lets through | Refuses | Key, flag | Where | Row |
|---|---|---|---|---|---|
| Hosts it may reach | The hosts listed, over `CONNECT` and `http://` | Any other host, by name, with a 403 and no lookup | `[internet] hosts`, `--internet.hosts` | both | `internet.hosts` |
| Any public host | Every public host | This machine, your network, a cloud's metadata service | `hosts = ["*"]` | both | `internet.any` |
| Names under a name | `*.example.com`'s names | `example.com` itself | `hosts = ["*.example.com"]` | both | `internet.wildcard` |
| Never this machine or its network | Public addresses | Loopback, private, link-local and metadata addresses | none | both | `internet.private` |
| HTTPS and HTTP only | `CONNECT` to 443, `http://` to 80 | Any other port no endpoint names | none | both | `internet.ports` |
| Named endpoints | A `host:port` its hosts allow, through the proxy | A port no endpoint names; a host not allowed | `[internet] tcp`, `--internet.tcp` | both | `internet.tcp` |
| DNS for named endpoints | Its hosts' names, its endpoints' at addresses of its own | Every other name, before anything is asked | `[internet] tcp` | Linux | `internet.dns` |
| Calls a minute | That many calls a minute | The rest, with a 429 | `[internet] per_minute`, `--internet.per_minute` | both | `internet.per-minute` |
| One host a tunnel | A TLS tunnel asking for the host checked | A tunnel whose ClientHello names another host | none | both | `internet.sni` |

## Your network and this machine

Machines of your network refuse this machine, its containers,
link-local and metadata addresses, and any public address. A local
model refuses pulling, creating, copying, pushing or deleting a model,
with a 403.

| Feature | Lets through | Refuses | Key, flag | Where | Row |
|---|---|---|---|---|---|
| Machines of your network | The names, addresses and ranges listed, on any port | This machine, its containers, link-local, metadata, public | `[lan] hosts`, `--lan.hosts` | both | `lan.hosts` |
| Cloud's network kept out | Exact addresses | `*` or a range holding a private block, on a cloud's machine | `[lan] hosts` | both | `lan.cloud` |
| This machine's ports closed | Nothing | Every port of this machine's loopback | none | both | `localhost.closed` |
| Ports it may call | The ports listed, the service's whole API | Every other port; `40781`; `"*"` behind a proxy | `[localhost] ports`, `--localhost.ports` | both | `localhost.ports` |
| Local model, inference only | Chat, generation, embeddings and the models' list | Pulling, making, copying, pushing or deleting, 403 | `[localhost] models`, `--localhost.models` | both | `localhost.models` |

## Routes

Ways out served outside the sandbox. A hosted model's key refuses a
request without the placeholder, a provider's server-side tools and a
URL to fetch. No web page reaches the proxy: a request to the proxy
itself from a page, with another `Host`, `Origin` or `Sec-Fetch-Site`,
is refused.

| Feature | Lets through | Refuses | Key, flag | Where | Row |
|---|---|---|---|---|---|
| Hosted model's key, added outside | Inference with the placeholder the run was given | No placeholder; server-side tools; a URL to fetch | an agent's keys on the wire | both | `routes.model` |
| Service's secret, added outside | The rule's paths with the placeholder | The rule's refused paths; any other host | an agent's secrets | both | `routes.secret` |
| Routes behind a secret | A call carrying the proxy's URL secret | A call without it, to a tool, a local model or `open` | none | both | `routes.url-secret` |
| No web page reaches the proxy | `CONNECT` and `http://` requests | A page's request to the proxy itself | none | both | `routes.web-page` |

## Tools and pages

Tools that run outside, the questions they ask you, and pages it may
open, which refuse another host or scheme, an app, a file, this machine
or its network.

| Feature | Lets through | Refuses | Key, flag | Where | Row |
|---|---|---|---|---|---|
| Tools it may call | The tools listed, at `$LNK_TOOL_<NAME>` | Every other tool | `[tools] names`, `--tools.names` | both | `tools.named` |
| Calls that don't ask | The calls the tool's `--add` names | Every other call, asked, refused with no one to ask | `lnk sandbox tool add --add` | both | `tools.allow` |
| Tool taken away | Nothing once removed | The next call of a running session | `lnk sandbox tool remove` | both | `tools.remove` |
| One server for every sandbox | Each session its own answers | One session's answers to another | `lnk sandbox tool add --shared` | both | `tools.shared` |
| Answering a question | `lnk sandbox answer` yes, once | No, two minutes of silence, a second answer | `lnk sandbox question list` | both | `asks.answer` |
| Pages it may open | An `https://` page on a host listed, asked first | Another host or scheme, an app, a file, this machine | `[browser] hosts`, `--browser.hosts` | both | `browser.hosts` |

## The wire

What the proxy logs and counts, and how its wire is stopped or paused.
A wire pauses itself after 30 refusals a minute, 20 targets, or a call
to a metadata service.

| Feature | Lets through | Refuses | Key, flag | Where | Row |
|---|---|---|---|---|---|
| Every call logged | A line a call: when, the sandbox, verb, host, outcome | A path, a header or a body | `lnk sandbox log show` | both | `wire.log` |
| Refusals logged | `refused` lines, alone with `--refused` | Other outcomes, with `--refused` | `lnk sandbox log show --refused` | both | `wire.refused` |
| Bytes each way | Each running proxy's counts | Nothing | `lnk sandbox show` | both | `wire.traffic` |
| Kill switch | Nothing while it's on; `--off` opens the wires again | Every call of every proxy, open ones within a second | `lnk sandbox stop` | both | `wire.stop` |
| One wire paused | Every call once resumed | Every call of that sandbox's proxy | `lnk sandbox stop <name>`, `start <name>` | both | `wire.pause` |
| Wire pausing itself | Normal work | Calls past 30 refusals a minute, 20 targets, metadata | none | both | `wire.auto-pause` |

## Specifications and checks

A specification file can't open Link's settings, a dot folder of your home,
`PATH`, the system's folders, the specification's own folder, sockets' folders,
or `..`. One shape refuses an unknown key, an empty section, `"*"`
beside other entries, and a file over 64 KiB.

| Feature | Lets through | Refuses | Key, flag | Where | Row |
|---|---|---|---|---|---|
| Specification from its grants | A specification of the flags given, printed | Nothing | `lnk sandbox spec show` | both | `spec.write` |
| New specification asks first | A specification run here before; `--yes` | A new or changed specification, no terminal or `--yes` | `lnk sandbox run --spec` | both | `spec.asks` |
| What a file can't open | A grant of a folder of your own | [What a file can't open](../../../config/specs/guides/sandbox.md#refused-in-a-file) | `--spec` | both | `spec.refused` |
| One shape, checked | Every key the table names | An unknown key, an empty section, a file too big | [`lnk spec show sandbox --json`](../../../../../../tooling/spec/docs/config/specs/guides/kinds/README.md#show-a-kind) | both | `spec.schema` |
| Flags over a file | A flag's key in place of the file's; `--files.hide` added | Nothing more | flags beside `--spec` | both | `spec.flags` |
| One OS's table | The table of the OS it runs on | The other OS's table | `[<section>.linux]`, `[<section>.macos]` | both | `spec.os` |
| Sandbox tried from inside | `blocked`, `allowed` and `open` rows | A `leaked` row, which exits 1 | `lnk sandbox check` | both | `check.probes` |
| Ready to run here | Exit 0 where it can | What's missing, said | [`lnk sandbox check --ready`](../../../../confine/docs/tooling/sandbox/wall/README.md#make-the-sandbox-ready) | both | `check.ready` |
| Session over SSH | A shell or command in its sandbox, its folder written | A specification whose `[commands]` doesn't run `/bin/bash` | [Box Users](../../../../../../compute/box/docs/services/boxes/user/README.md) | Linux | `enter.session` |

## Its permissions

`lnk sandbox permission list` lists each section of a specification and its flags,
and `lnk sandbox run --help` describes each flag. An agent's grants,
which [`lnk agent permission add`](../../../../../../agents/agent/docs/tooling/sandbox/permissions/README.md#choose-what-your-agent-can-reach) and `deny` change, are each a key of these rows:
[Permissions](../../../../../../agents/agent/docs/tooling/sandbox/permissions/README.md).
