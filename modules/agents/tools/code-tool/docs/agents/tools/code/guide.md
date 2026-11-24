# Code Guide

Enable Code to ask installed language servers for diagnostics, definitions, and references. Choose the servers and project access in your agent’s specification and sandbox file.

```sh
lnk agent start
lnk agent tool add code
lnk agent tool list
```

## Ask it about a file

Ask it "what's wrong in work/app/src/main.rs?" and it calls
`file__diagnostics`, which starts `rust-analyzer` in the crate, unless
it runs there already, and answers with each error and its line.

### What it needs

You need the agents modules, which [`lnk up`](../../../../../../../../docs/more/modules/install.md#pick-your-modules) installs and which
bring Link Code, and Link Harness. A new agent has `code` off:
[`lnk agent tool add code`](../../../../../../agent/docs/agents/README.md#add-tools-and-skills) turns it on, and it gets `code` at its start
when a language server it may run is on your computer. The language servers run locally without a search or analysis API fee. Your agent’s model calls retain their usual cost.

### Marked as your files'

A server's diagnostics, and the lines a definition or a reference shows, comes inside a fence, an opening `<untrusted ...>` line and a closing line with the same random ID, which tells your agent's model that it's data to read and never your instructions, since anyone may have written it. [Untrusted Content](../../../../../../../../packages/untrusted/docs/more/libraries/runtime/untrusted.md) explains the fence.

## The servers

Link knows seven, and gives your agent each one your computer has. Each
is named by its language, as the specification's `[tool.code.servers]` names it.

### Languages

Seven languages each have their server, started for the files it reads.

#### `rust`

`rust-analyzer`, for `.rs` files, its project's root the folder holding
`Cargo.toml`, its builds run with `cargo` and `rustc`.

#### `go`

`gopls`, for `.go` files, its project's root the folder holding
`go.mod`, its builds run with `go`.

#### `python`

`pyright-langserver --stdio`, for `.py` and `.pyi` files, its project's
root the folder holding `pyproject.toml`, `setup.py` or
`pyrightconfig.json`.

#### `typescript`

`typescript-language-server --stdio`, for `.ts`, `.tsx`, `.js`, `.jsx`,
`.mjs` and `.cjs` files, its project's root the folder holding
`tsconfig.json`, `jsconfig.json` or `package.json`.

#### `c`

`clangd`, for `.c`, `.h`, `.cc`, `.cpp`, `.cxx`, `.hpp` and `.hh` files,
its project's root the folder holding `compile_commands.json` or
`CMakeLists.txt`. On a Mac, `/usr/bin/clangd` runs Xcode's or the
Command Line Tools' through `xcrun`, which the server's sandbox reaches
while the sandbox file's `[commands.macos] developer_tools` is on, as
it is in Link's file.

#### `java`

`jdtls`, the Eclipse Java language server, for `.java` files, its
project's root the folder holding `pom.xml`, `build.gradle`,
`build.gradle.kts`, `settings.gradle` or `settings.gradle.kts`. Its
launcher is a Python script that starts a JDK 21 or newer, so it needs
both toolchains' programs, and it keeps its index under `~/.cache/jdtls`,
which the sandbox's home holds only while the server runs. With no
network, it reads a Maven or Gradle project's sources but fetches none of
its dependencies.

#### `kotlin`

`kotlin-language-server`, for `.kt` and `.kts` files, its project's root
the folder holding `settings.gradle.kts`, `settings.gradle`,
`build.gradle.kts`, `build.gradle` or `pom.xml`. It starts a JDK too, and
like `java` reads the project's sources but fetches no dependencies.

### Found on the `PATH`

A server is found as `run`'s programs are, on the `PATH` of whoever last
ran [`lnk agent start`](../../../../../../agent/docs/agents/README.md#create-an-agent), then the system's, and only if your agent's
sandbox file's `[commands]` allows it. Link's file allows any program
on the `PATH` (`allow = ["*"]`) beside its toolchains, so all seven and
the builds they run; drop `allow` to keep only the toolchains' programs.

### A server in your home folder

A server installed in your home folder (rustup's `rust-analyzer`,
`uv`'s `pyright`) [is found on your `PATH` too](../../../../../../agent/docs/agents/more/settings/README.md#where-it-finds-programs), and read in its sandbox
with what its toolchain needs beside it.

### Change one, or add a language

Change one, or add a language, in your agent's specification. Each key is its
own: a specification that names only `command` keeps Link's `files` and `roots`.

```toml
# ~/.lnk/safe/agents/main/spec.toml
schema = 1
[tool.code.servers.rust]
command = ["/home/me/.cargo/bin/rust-analyzer"]

[tool.code.servers.zig]
command = ["zls"]
files = ["zig"]
roots = ["build.zig"]
```

### Give a server settings

`settings` gives a server its settings, by the section it reads, the way
an editor would. The server is told them once it starts, and is given
each section it asks for. Link gives `rust-analyzer` one setting, so a
symbol is searched among functions and constants as well as types.

```toml
# ~/.lnk/safe/agents/main/spec.toml
schema = 1
[tool.code.servers.rust]
settings = { rust-analyzer.workspace.symbol.search.kind = "all_symbols" }
```

### Turn one off

`command = []` turns a language off. Two languages can't serve one
extension: a specification that gives one extension to two is refused, naming
both.

## How your agent calls it

Your agent sees three language-server actions in the `file` namespace. Each answers `path:line:column:` and the
line, lines and columns counted from 1. The line is trimmed and shown
at most 200 characters long, and only from a file in the files folder. A
place elsewhere, such as a toolchain's file, is shown by its path alone,
from `~` when it's in your home folder.

### Bounded answers

An answer lists at most 200 places or problems and 32 KB of them, a
diagnostic's message at most 2,000 characters, and counts the rest
(`…and 12 more`). A `character` past its line's end is refused, naming
the line's length.

### Diagnostics

In Link Harness, `/file:diagnostics` invokes this action with its JSON
arguments, using the enabled Code provider and its grants.
A file's errors and warnings (`file_path`), each with its line; without one, every
problem the servers started so far report.

### Definitions and References

In Link Harness, `/file:definition` and `/file:references` invoke these
actions with the corresponding JSON arguments and Code provider grants.
`file__definition` gives where a name is defined: given its `file_path`,
`line` and `character`, as the language-server tool models know takes them, or
its symbol (`helper`). `file__references` gives every place a
name is used, its definition with them, found the same way.

### Several Symbols of One Name

A symbol is matched by its exact name. When several share it, such as
two types' `new`, `file__definition` lists each, saying how many, and
`file__references` refuses, naming each place. Give the `file_path`, `line`,
and `character` of the one you mean.

### When a server starts

A server starts the first time a file of its language is asked about, in
the nearest folder above the file holding one of its root files, else
the files folder; a project with two roots gets a server each. Each
stays while your agent's session with the tool does, and sees a file as
it is on disk when it's asked about. Nothing watches your files, so each
call gives the server every file it was given before as it is now,
closes one that's gone, and tells it which other files of its language
under its root were created, changed, or deleted.

### How long a call waits

A call waits on its server at most `[tool.code] timeout` (your agent's
`[tools] timeout`, 5 minutes), a tenth less (2 s at least). Diagnostics wait until the
server is quiet. A definition or references call waits half that time
for the server to finish its work, then asks, leaving the rest for its
answer. An answer from a server still working says it may not be all,
and its result's `complete` is `false`.

## Each server in a sandbox

A language server runs the project's code: build scripts, macros,
modules. So each runs as `run` runs a command, in a sandbox made from
your agent's sandbox file: it reads the files folder, writes only the
folders its `[files]` lets it write, starts only the programs of its
`[commands]`, and reaches no network. A server outlives the call that
started it, so it reads no conversation's folder in `chats/`. The tool reads only files in the
files folder, and shows a line only from there, whatever a server names.

## Terminal Actions

[`lnk agent file`](../../../../../file-tool/docs/agents/tools/file/guide.md#terminal-actions) exposes language-server actions for the selected agent. Read an action’s JSON arguments from a file with `--request <file>`, or from standard input when the flag is omitted. The action uses the running agent’s configured tool and grants.

### Choose an Action

`lnk agent file definition`, `lnk agent file references`, and `lnk agent file diagnostics` use the corresponding calls described above. Use `--agent work` to select another agent and `--json` for machine-readable output.

## More about it

[Agents
Security](../../../../../../agent/docs/agents/tools/use/security.md#code)
has the guarantees and gaps. To work on its code, see [Developing Link
Code](more/developing.md), and when your agent has no `code` or a server doesn't
answer, [Troubleshooting](troubleshooting.md).
