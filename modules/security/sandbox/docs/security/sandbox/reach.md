# Choose What the Sandbox Can Reach

The programs [a sandboxed program](README.md) runs, the hosts, machines and models it
reaches, the tools it calls and the pages it opens, each a flag of `lnk
sandbox run` and a key of a specification.

## Choose the programs it may run

Nothing runs that `[commands]` does not name, whether the grants come
from a file or field flags. The program given to `run` must be named,
or the run is refused before it starts. A grant always says what it lets
run, so no entry stands for every program: `allow = ["*"]` is refused.

```sh
lnk sandbox learn --spec app.toml -- npm test
lnk sandbox run --files.write . --commands.allow sh --commands.allow git -- sh -c 'git status'

lnk sandbox run --spec app.toml -- npm test
```

### A program refused

A program started from inside that isn't listed gets "Permission
denied", and the sandbox says which:

```text
lnk sandbox: refused /usr/lib/git-core/git-remote-https: add it to [commands] allow
```

### Toolchains

A toolchain names a language's or a task's programs in one word:
`--commands.toolchains rust` or `toolchains = ["rust", "git"]`. It
stands for its programs found on `PATH` (Cargo, rustc, and
rust-analyzer for Rust), and the folders where those keep the programs
they start themselves (rustup's toolchains, the compiler's own).
The sandbox specification's
[toolchains field](../../config/specs/sandbox.md#toolchains) lists every
toolchain and what each one grants. A name it doesn't list is refused
when the specification is read. `lnk sandbox permission list` lists each
toolchain with its programs, as the sandbox declares them.

#### Toolchains in the Home Folder

A sandbox's home folder holds only what it's granted, so a toolchain
installed there is granted read where it is. Rustup puts Cargo in
`~/.cargo/bin`, as a link to `rustup` named `cargo`, and its toolchains
in `~/.rustup/toolchains`. The sandbox reads the folder each program is
found in and each declared folder in the home folder. Only the program a
link resolves to runs, and nothing there is written.

#### Build folders

A toolchain also runs what its builds make, in each folder the sandbox
writes that is a project of its kind. Rust's build scripts and tests run
from `target/` beside `Cargo.toml`, a package's commands from
`node_modules/` beside `package.json`, and a virtual environment's from
`.venv/` beside `pyproject.toml` or `requirements.txt`. A folder of
another kind gets no build folder. A build folder is made as the sandbox
starts, since a folder that isn't there can't be granted, and never
through a link.

##### Installer build folders

A program wrapping a sandbox with `lnk sandbox wrap` may name one more
build folder: the folder an installer it runs installs into, which the
sandbox writes. An installer that unpacks a runtime there [can then run
it](security.md#build-folders). A specification never names one.

```sh
lnk sandbox run --files.write . --commands.toolchains rust --commands.toolchains git -- cargo test
```

### `learn`

`learn` runs the command once under the specification's other grants with every
program open, and prints the entries to add to `[commands] allow`. It
writes nothing. A helper a program starts only now and then is found
only if this run starts it.

### An entry

An entry is a name found on `PATH`, a path, a folder ending in `/` for
every program in it. A script needs itself and what it
starts with, so `#!/usr/bin/env node` needs `env` and `node`. An entry
in a folder it may write, or in `/tmp`, is refused. A toolchain's build
folders are the one exception.

### An interpreter

Listing an interpreter such as `sh`, `node` or `python3` lets it run any
code it's given. The list limits which programs run, not what they do.

### What it needs

On Linux this needs Landlock, which is in Linux 5.13 and newer. On
macOS, naming a refused program and `learn` need an admin user, because
they read what Seatbelt reports in the system log.

## Give it the internet through Link's proxy

The program gets the internet without the rest of your network. Its
only way out is Link's filtering proxy, set as its `HTTPS_PROXY`, which
refuses this machine, your network and a cloud's metadata service. Every
agent with `[internet]` runs this way, unless it has `[network] mode =
"host"`.

```sh
lnk sandbox run --commands.toolchains python --internet.hosts '*' -- python3 fetch.py
lnk sandbox run --commands.allow ./bot --internet.hosts api.anthropic.com --internet.hosts '*.telegram.org' -- ./bot
lnk sandbox run --commands.toolchains git --internet.hosts github.com --internet.tcp github.com:22 --files.write . -- git clone git@github.com:me/repo.git

lnk sandbox run --commands.toolchains node --internet.hosts '*.abc.mongodb.net' --internet.tcp '*.abc.mongodb.net:27017' -- node app.js
lnk sandbox run --commands.toolchains shell --internet.hosts '*' --upstream socks5://127.0.0.1:1080 -- curl -sI https://example.com
```

### `--internet.hosts` and `--internet.tcp`

`--internet.hosts` names a host it may reach, or `*` for any, and
`*.telegram.org` covers the names under it but not `telegram.org`
itself. `--internet.tcp <host:port>` opens an endpoint beyond HTTPS on 443 and
HTTP on 80, such as `github.com:22` for git over SSH, and its host must
be one it may reach.

### `--upstream` and `--internet.per_minute`

They change where its traffic leaves from and how many calls it may
make. The upstream is `direct`, a SOCKS5 proxy
(`socks5://127.0.0.1:<port>`, or `socks5://<a socket's absolute
path>`), or an HTTP proxy on this machine's loopback
(`http://127.0.0.1:<port>`).

### An HTTP proxy upstream

It's for a machine whose traffic leaves only through a filtering proxy
of its own network: that one is handed each host's name, which it
resolves, so this machine looks up no name the sandbox asks for.

### A program that ignores `HTTPS_PROXY`

It has no network. Python follows it, and Node gets
`NODE_USE_ENV_PROXY=1`. Git over SSH works once its endpoint is named
with `--internet.tcp`, using the sandbox's own key in its `~/.ssh`.

### Names on Linux

On Linux [the sandbox also gets DNS from its proxy](../../../proxy/docs/security/sandbox/more/proxy/connect.md#dns-where-a-sandbox-names-endpoints-linux), so a `--internet.tcp` host
with a port from 1024 is reached by its name, and `psql
"host=db.example.com"` and `mongodb+srv://` work as they are.

### Other endpoints, and a Mac

Other endpoints, and every one on a Mac, are reached only by a client
that takes a `ProxyCommand`, like ssh. On a Mac, `psql` and database
drivers can't reach a `--internet.tcp` endpoint.

## Reach your network

`--lan.hosts` lets it reach machines of your network on any port, through
its proxy. It takes a name, an address, a range, or `*` for the private
ranges, and your network is reached from here whatever `--upstream`
says. [Security](security.md#what-can-cross-the-sandbox) says which
addresses a host reaches, and what it never reaches whatever is listed.

```sh
lnk sandbox run --commands.toolchains shell --lan.hosts nas.local -- curl http://nas.local:5000/
lnk sandbox run --commands.allow ./sync --lan.hosts 192.168.1.0/24 --internet.hosts '*' -- ./sync
```

## Let it use a model

`--localhost.models <port>` makes a local model runtime such as Ollama, LM Studio
or llama.cpp reachable for inference only. That means chat, generation,
embeddings and the models' list, and pulling, creating or deleting a
model gets a 403. On Linux the program keeps the runtime's address. On
macOS it reaches the runtime at `$LNK_LOCAL_MODEL_<port>`, on its proxy.

```sh
lnk sandbox run --commands.toolchains python --localhost.models 11434 --files.write . -- python3 chat.py
```

### A hosted model's key, or a secret

The proxy can also add a hosted model's key, or a service's secret such
as a Telegram bot's token, so the program never holds it: each is a
route of the proxy's, served outside the sandbox by the module it's for
(`lnk-model`, `lnk-secret`), as [Security
explains](../../../contract/route/docs/security/sandbox/more/contract/models-and-secrets.md#routes-served-outside).

### For an agent

Agent startup configures this route automatically on Link Harness and on
harnesses whose adapter accepts the proxy address. The
[route interface](../../../contract/route/docs/security/sandbox/more/contract/models-and-secrets.md)
explains how that startup passes the permitted model credential use.

### Calling it

The program calls `http://127.0.0.1:<proxy port>/_link/model` with a
placeholder key, and adds `/v1` for OpenAI and OpenRouter. The proxy
port is `40781` on Linux. It works with Anthropic, OpenAI and
OpenRouter. A service's secret is reached the same way, as [Security
explains](../../../contract/route/docs/security/sandbox/more/contract/models-and-secrets.md#a-services-secret-added-by-the-proxy).

## Call a tool that runs outside

A tool is an MCP server that runs outside the sandbox, with your logins,
desktop and SSH agent, or a server at a URL reached from outside with
its headers, which no sandbox sees. Use one for what the sandbox keeps
out, such as `gh` or `aws` with your credentials. A sandbox reaches only
the tools it names, at `$LNK_TOOL_<NAME>`, so `$LNK_TOOL_GH` for `gh`.

```sh
lnk sandbox tool add gh --add list_issues --add get_issue -- github-mcp-server stdio
lnk sandbox tool add docs --url https://example.com/mcp --header-file docs.headers

lnk sandbox tool add notes --shared -- notes-mcp serve
lnk sandbox tool-call list gh
lnk sandbox tool list
lnk sandbox run --commands.allow my-agent --tools.names gh -- my-agent
lnk sandbox tool remove gh
```

### Asked first

Every call asks you first, except those you `--add` by name, and
`--ask none` asks for none. Removing a tool ends its sessions.

### Asked by an Argument

`--ask-if <call>.<argument>` makes a call ask by one of its boolean
arguments: it asks unless that argument is given `false`, even when
`--add` names the call. A tool whose call takes a flag, such as a
schedule's `recurring`, asks for the one case and lets the other
through.

```sh
lnk sandbox tool add scheduler --add schedule --ask-if schedule.recurring -- lnk-scheduler-tool serve
```

The argument is read from the call as the tool receives it, so a call
that leaves it out asks. Since `--add` can't let such a call through, `--ask-if-hint` says what
can, in the words of whoever adds the tool, such as an owner's setting.
A call it asks by that's refused [with no one to answer](tools/questions/README.md#with-no-one-to-ask) names the hint,
and without one names the argument to give `false`.

```sh
lnk sandbox tool add scheduler --add schedule --ask-if schedule.recurring --ask-if-hint "recurring = \"allow\" in its settings lets it through" -- lnk-scheduler-tool serve
```

### `--shared`

A tool with `--shared` runs one server for every session, where any
other starts one each, and stops once none has been open for the
sandbox's `shared_idle`: a team's memory is one.

### `--env`

A tool's server runs with a few of your variables as they were when it
was added, and `--env` gives it one more: `NAME=value`, or `NAME` for
its value here at that moment. Only the server gets it. The sandbox
calling the tool never sees it, and `lnk sandbox tool list` never prints
it, so a token for the server belongs here.

```sh
lnk sandbox tool add aws --env AWS_PROFILE=work -- aws-mcp-server
```

A secret goes by its name alone. A command line can be read by other
users of this machine and stays in your shell's history, so `--env`
refuses `NAME=value` for a name that says it holds a secret (`…_TOKEN`,
`…_KEY`, `…_SECRET`, `…_PASSWORD`). Set it in your shell without
showing it, as `read -rs GITHUB_TOKEN` then `export GITHUB_TOKEN` do,
then name it.

```sh
lnk sandbox tool add github --env GITHUB_TOKEN -- github-mcp-server stdio
```

## Open a page in your browser

With `--browser.hosts`, a program can open an `https://` page in your
browser, such as a login, on the hosts given (`*` for any). Every page
asks you first and shows its whole URL, because it opens with your
cookies and logins. `$BROWSER` in the sandbox does it, so Python's
`webbrowser`, `gh` and `xdg-open` work as they are.

```sh
lnk sandbox run --commands.toolchains python --browser.hosts github.com -- python3 -m webbrowser https://github.com/login
```

### A login's answer

A login that answers on `localhost` (its `redirect_uri`) gets that port
until the answer arrives, or for five minutes at most. With no desktop,
such as on a box, nothing opens, and the program's own fallback, a URL
to copy or a code to type, still works.
