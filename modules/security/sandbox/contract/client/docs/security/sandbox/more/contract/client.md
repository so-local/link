# Interface

What other programs can rely on from `lnk sandbox`.

## `lnk sandbox`

A module confining a program makes a `link_sandbox_client::Confinement`
from a specification (`Confinement::of`) and calls `link_sandbox_client::wrap`
(`lnk sandbox wrap`, installing the sandbox module first if it's
missing), then runs the command it gets with the environment it
chooses. The sandbox decides only what the program may touch; what
runs, where and with which environment stays the caller's.

### The contract and the specification

The contract's source is
[`sandbox.rs`](https://github.com/so-local/link/blob/main/modules/security/sandbox/contract/client/src/sandbox.rs).
[A sandbox's specification](../../../../../../../docs/config/specs/sandbox.md), `link_sandbox_client::Spec`, is the type a module keeps
one as; what it opens is the sandbox module's to say, by running it.

## `lnk sandbox wrap`

Reads `{"confinement", "argv"}`, or `{"unwalled", "argv"}`
(`link_sandbox_client::WrapRequest`), on stdin, and prints `{"argv",
"env"}`: the command running `argv` confined to it on this OS, and what
to add to its environment.

### What it refuses

An unknown key in the confinement, its `net` or its `proxy`, or a policy
(`{"policy", "argv"}`, as callers sent before confinements), is refused.

### What it adds to the environment

With a proxy, `HTTPS_PROXY` and the rest, and each route's `env`: a
tool's address, `LNK_TOOL_<NAME>`, a local runtime's,
`LNK_LOCAL_MODEL_<PORT>`, and with `open`, `BROWSER` and its address,
`LNK_SANDBOX_OPEN`. Those of a route that needs it carry `secret`, 16 to
64 letters and digits the proxy requires on it, or one of the wrap's
own.

### What it writes

Its rules to a unique profile under `~/.lnk/safe/sandboxes/` (macOS), its
routes and their caller to `<name>.routes.json`, and `secret` to
`<name>.url-secret` beside them.

### Who reads it

`lnk-agent`: every step of a harness (`install`, `configure`,
`command`, `health`, the harness); with no sandbox, the harness alone,
unwalled, and [`lnk agent harness run`](../../../../../../../../../agents/agent/docs/agents/harnesses/use/harness/README.md#inspect-the-sandbox).

## A confinement

`link_sandbox_client::Confinement`, made from a specification by
`Confinement::of`: `{"name", "write", "write_dot_files"?, "read",
"read_home"?, "hide"?, "tmp_prefix"?, "terminal"?, "commands",
"developer_tools"?, "net"}`, `commands` `"any"` or `{"only":
[<path>]}`.

### What stays closed

Link's settings stay closed whatever the confinement says; `hide`
closes folders the same way, but for what `write` and `read` name in
them.

### Dot files

At the top of each of `write`, a project's dot files that run outside
(`.git`, `.envrc`, `.vscode`, `.idea`) are read only, but in those
`write_dot_files` names.

### Its `net`

A confinement's `net` is one mode holding only its own keys:
`"closed"`, `{"loopback": {"ports", "dir"}}`, `{"proxied": {"proxy",
"ports"}}`, `{"reaching": {"reached": [{"dir", "port"}], "ports"}}` or
`{"host": {"ports", "localhost"}}`.

### `loopback`

Its `dir` a folder: it reaches its `ports` on this machine's loopback
and nothing else, without a proxy (on Linux, relayed through sockets in
that folder).

### `reaching`

Linux: a network of its own whose only ways out are those proxies, each
at its `port` inside, through the sockets in its `dir` (`lnk sandbox
proxy-inside --also <dir>=<port>`): one sandbox for several agents'
proxies.

### `reaching`'s folders

`lnk sandbox wrap` keeps a `reached` folder that's its user's with mode
`2770` as it is, and its relay inside makes `in.sock` `0660`, of the
folder's group.

## A proxy

A proxied confinement's `proxy` (`link_sandbox_client::Proxy`) is
`{"upstream", "dir", "serve"?, "hosts"?, "tcp"?, "listen"?,
"per_minute"?, "routes"?, "caller"?, "secret"?, "lan"?,
"no_internet"?, "hold"?}`. On macOS the proxy listens on `listen`, else
binds port zero itself before starting its program.

### Pointed at it

For a proxy on this machine's loopback (macOS, `unwalled`), with
`pointed` (a sandbox on macOS always), the wrap makes a new secret of
its own, which `HTTPS_PROXY` alone names
(`http://lnk:<secret>@127.0.0.1:<port>`) and the proxy reads there
(`--pointed-by HTTPS_PROXY`).

### `Proxy-Authorization`

The proxy requires that secret as the `Proxy-Authorization` (`Basic`,
any user) of every `CONNECT` and plain `http://` request, answering
`407` without it.

### `hold`

Linux: the command is the proxy's alone (`lnk sandbox proxy --hold`),
holding no program until stopped, and the environment is the program's
as it reaches the proxy at `listen` from a sandbox started apart. `tcp`
and ports to reach are refused, and a local model is reached on the
proxy, at `local_model_url`.

### Port zero in `Wrapped.env`

For a loopback proxy with no fixed `listen`, generated proxy URLs in a
`Wrapped.env` use port zero as a launch placeholder. Run the returned
command with that environment. The proxy owns its listener before
filling the actual port in its child's generated and inherited run
URLs; those placeholder URLs are not addresses to call before launching
the command.

## Routes

Each of a proxy's `routes` (`link_sandbox_route::Route`) is a way out
beside the network, mounted by the proxy and served by a bridge outside
the sandbox: what each is, the routes Link makes and their bridges are
on [Routes](../../../../../../route/docs/security/sandbox/more/contract/routes.md).

## Unwalled

`unwalled` (`link_sandbox_client::Unwalled`, made from a specification by
`Unwalled::of`) is `{"name", "proxy", "pointed"}`: no wall, `argv` run
as it is under its proxy (`lnk sandbox proxy --listen`) on this
machine's loopback at the proxy's `listen`, its `dir`, `serve` and
`hold` unread. Nothing holds the program to it.

### Its listener

None, a port it binds before starting the program. A listener there its
caller holds from choosing the port, `link_sandbox_client::HeldPort`, is
handed to it open, its descriptor named by `LNK_PROXY_LISTENER_FD`, and
served as it is, never bound again. The proxy refuses one that isn't a
TCP socket on `127.0.0.1:<listen>` or can't listen (one connected), and
asks whether it listens by listening, since macOS answers no
`SO_ACCEPTCONN` question. Its program gets neither the listener nor the
variable.

### `pointed`

With `pointed`, the environment points it at the proxy
(`HTTPS_PROXY`, `GIT_SSH_COMMAND`), else only at Link's ways out on it
(`LNK_TOOL_<NAME>`, `LNK_LOCAL_MODEL_<PORT>`, `LNK_SANDBOX_OPEN`).

### What it refuses

An `upstream` but `direct` is refused, and [`lnk sandbox check --policy`](../../../../../../../docs/security/sandbox/guide.md#sandbox-guide)
refuses it, having no wall to check.

## A blank proxy

`lnk sandbox proxy --blank --control <socket> --dir <dir> [--binder
<uid>] [--agent-user <uid> --group <gid>] (--hold | -- <argv>)`
(hidden, Linux): a proxy holding nothing of anyone's, for a machine
that packs agents, never with `--listen` or `--dns`. It listens on
`<dir>/out.sock` answering every call with a 503.

### Its binding

On `<socket>` (0600, its user's connections only, and `<uid>`'s with
`--binder`) it waits for one line of JSON, `link_sandbox_client::ProxyBind`
(`{"name"?, "wire"?, "upstream", "host"?, "tcp"?, "serve"?, "reach"?,
"desktop"?, "lan"?, "no_internet"?, "per_minute"?, "routes"?,
"caller"?, "url_secret"?}`): what `lnk sandbox proxy` takes, its routes
as `wrap`'s, what each is given, their caller's key and the URL secret
inline, never in a file.

### Its answer

`link_sandbox_route::check_routes` refuses routes that can't be mounted
together, and a bind with routes and no caller is refused. It answers
one line, `{}` or `{"error"}` (`ProxyBound`;
`link_sandbox_client::bind_proxy` sends it). Bound, it's the proxy those
arguments start; a second bind is refused, and the binding ends with the
process.

### `--agent-user` and `--group`

With them (any `lnk sandbox proxy --dir`, blank or not), it runs as a
user other than its agent's: `<dir>` must be of group `<gid>`, mode
`2770`, or it doesn't start; its sockets there are made `0660` of that
group; and it takes `in.sock` only as a socket of `<uid>`'s.

## `lnk sandbox run`

`lnk sandbox run --yes --files.write <dir> --files.write_dot_files <dir>
--internet.hosts <host> -- <argv>` runs `argv` in a sandbox writing only
`<dir>` (its dot files too) and reaching only `<host>` on the internet,
and exits with its code.

### `--spec` and `--with-fd`

[`lnk sandbox run --spec <file> --yes --with-fd <N> -- <argv>`](../../../../../../../docs/security/sandbox/guide.md#sandbox-guide) runs
`argv` in the specification's sandbox as `run` does, first reading
`link_sandbox_client::RunWith` as JSON from descriptor `N` to its end:
`{"env", "secrets": [{"secret", "url", "placeholder", "git"?}],
"serve", "write"}`.

### `env`

Variables the program gets: none `run` keeps or sets, nor `LD_`, `DYLD_`
or `_PROXY` ones. Its program and file and network grants still
determine what code it explicitly asks to run can do.

### Refused in `env`

It refuses its own variables, and dynamic-loader, shell-startup and
interpreter-injection ones: `BASH_ENV`, `ENV`, `NODE_OPTIONS`,
`NODE_PATH`, `PYTHONPATH`, `PYTHONHOME`, `PYTHONSTARTUP`, `PERL5OPT`,
`PERL5LIB`, `RUBYOPT`, `RUBYLIB`, `JAVA_TOOL_OPTIONS`, `_JAVA_OPTIONS`,
`JDK_JAVA_OPTIONS`, `GCONV_PATH` and `LOCPATH` among them.

### `secrets`

Secrets its proxy adds, each a `Secret` as `wrap` takes it, but for its
`placeholder`, which `run` makes for this run. The variable `url` names
holds `secret_url`, the one `placeholder` names that placeholder.

### `git`

With `git`, for a secret in a header, git's own configuration is in the
variables too: `GIT_CONFIG_COUNT` with `url.<secret_url>/.insteadOf
https://<host>/` and `http.<secret_url>/.extraHeader` holding the header
with the placeholder, refused beside a `GIT_CONFIG_` variable of the
caller's.

### `serve` and `write`

Ports it serves on this machine's loopback too, and folders, in full,
it writes beside its specification's, each refused [as a specification's write would be](../../../../../../../docs/config/specs/guides/sandbox.md#refused-in-a-file). A proxy with
no internet is made for secrets or ports when the specification opens none.

### Handed, never in a file

Its proxy is handed its routes, the secrets' among them, on a pipe as it
starts (`lnk sandbox proxy --routes-handed <variable>`, read by
`link_secretive::Secret::from_env`), never in a file. The rest of what it
was given is removed when it ends, stopped by SIGTERM or SIGHUP too
(each passed on to what it runs).

### Stops on Linux

Bubblewrap passes on no signal, and a stop sent to it ends the sandbox
before the program hears of it. So the stage in front of bubblewrap asks
it for its report (`--info-fd`), which names the sandbox's first
process, and sends each stop to the process group that process leads in
the sandbox's own session. Each of Link's stages inside passes it on to
the program.

### Only the sandbox's group

The group is named through a pidfd (a descriptor that holds one
process, never another given its number later), opened only once that
process is proved bubblewrap's child, so a stop reaches nothing outside
the sandbox. Before the group exists, after it ends, or on a kernel
older than Linux 6.9 (which can't signal a group through a pidfd), the
stop goes to bubblewrap, ending the sandbox.

## Other commands

What [`lnk measure`](../../../../../../../../../tooling/measure/docs/tooling/measure/README.md#measure) and [`lnk agent`](../../../../../../../../../agents/agent/docs/agents/README.md#agents) read of what a sandbox carried, its
state and its readiness.

### `lnk sandbox show --json`

`[{"pid", "sandbox", "up", "down"}]`: what each running sandbox's proxy
(its process, `pid`) has carried since it started, every call it let
through, in bytes from the sandbox (`up`) and to it (`down`).

### `lnk-sandbox answer measure.traffic`

The same list, `link_measure_contract::client::asked::Traffic`, as this
module's answer to `lnk measure`'s `measure.traffic` (an agent's
network: its proxies' among its processes), asked of its program
directly and [answered in its envelope](../../../../../../../../../../core/framework/docs/more/modules/protocol.md#questions).

### Published counters

The proxy also publishes these typed records under `sandbox/traffic/` in
the user's private settings, once per PID with its start marker. Measure
uses the shared reader directly when an agent advertises that source. The
proxy retains publication cadence and exit cleanup. The sandbox command and
the next proxy startup remove stale files, while observation stays read-only.

### An agent's model tokens

Its model route's (`lnk-model bridge`),
`link_measure_contract::client::Tokens`, `{"requests", "unmetered",
"input", "output", "cached", "written"}`: the requests for a model's
answer it carried that were answered, and of them those whose answer
gave no usage.

### The tokens

The rest's usage says the prompt's tokens, cached or not (Anthropic's
three parts summed); the answer's; and of the prompt, those read from
the provider's cache, and those written to it.

## Proxy and Machine Status

Callers inspect pause state and machine support independently of traffic
measurements. These status and callback commands retain their own response shapes.

### `lnk sandbox list --json`

`{"stopped", "sandboxes": [{"sandbox", "state", "paused"}]}`: whether
every wire is stopped, and each sandbox (`<agent>-<harness>`, a box
user's name, `run-<pid>`) this machine knows, `running` while its proxy
runs or `ended` once only its log is left, with why its wire is paused
when it is. `--paused` keeps only those paused.

### `link_sandbox_client::Wires`

`link_sandbox_client::Wires`, `{"stopped", "paused": [{"sandbox",
"why"}]}`, reads the same owner-defined flags afresh without a child
command, as [`lnk agent show`](../../../../../../../../../agents/agent/docs/agents/models/agent/README.md#set-its-model-and-how-hard-it-thinks) does. Missing
state is open, unreadable state is unavailable, and reading creates no
directory. This observation never replaces the proxy's enforcement checks.

### `lnk sandbox on-stop <name> [-- <args>]`

Hidden: `lnk <args>` runs when [`lnk sandbox stop`](../../../../../../../docs/security/sandbox/guide.md#stop-a-sandbox) closes the wires, for
`<name>`, kept in `~/.lnk/safe/sandboxes/on-stop/<name>.json`; with no args,
nothing does anymore. Never [`lnk sandbox`](../../../../../../../docs/security/sandbox/README.md#sandbox) itself.

### Who sets it

[`lnk agent start`](../../../../../../../../../agents/agent/docs/agents/README.md#create-an-agent) (`agent-<agent>`: `agent stop -a <agent>`, so the
kill switch stops an agent with `[network] mode = "host"` too, which has
no wire), and it's forgotten when the agent leaves.

### `lnk sandbox check --ready [--yes]`

Exit status 0 once the sandbox can run here, checked afresh (what was
kept of this machine forgotten); in a terminal, offers what it needs
(Ubuntu's AppArmor profile), talking on stdout. With `--yes`, asks
nothing and adds it where `sudo -n` works (installing bubblewrap with
apt if it's missing).

### Who reads it

`lnk agent start`, [`lnk agent harness check`](../../../../../../../../../agents/agent/docs/agents/harnesses/use/harness/README.md#inspect-the-sandbox), and [`lnk agent move`](../../../../../../../../../agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud) and
[`lnk agent create --on`](../../../../../../../../../agents/agent/docs/agents/start/create.md#create-on-a-hosting-account) readying a box with `--yes`, through [`lnk box run`](../../../../../../../../../compute/box/docs/services/boxes/ssh.md#run-lnk-on-a-box).

### `lnk sandbox tool list --json`

One JSON object per tool that runs outside: `{"name", "argv", "ask",
"allow"}` (`ask`: `all` or `none`; `allow`: the calls that don't ask). A
proxied confinement's proxy's `tools` names some of them; its program
finds each at `$LNK_TOOL_<NAME>`. Read by [`lnk agent permission add --tools.names`](../../../../../../../../../agents/agent/docs/security/sandbox/permissions/README.md#choose-what-your-agent-can-reach), and
`lnk agent start` (a tool removed since is left out).

### `lnk sandbox check --policy`

Reads `{"confinement"}` on stdin (`link_sandbox_client::Wrap`), as
`wrap` takes it (a proxied one too), the flag keeping its name, runs the
probes inside it, prints a line per probe (`blocked`, `allowed`,
`failed`, `open`, `missed`, `leaked`), and exits 1 when something leaked. Read by
`lnk agent harness check`.
