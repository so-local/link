# Routes

`link-sandbox-route`: a route, [a way out of a sandbox beside the
network](../../../../../../client/docs/security/sandbox/more/contract/client.md), which its proxy mounts under `/_link/` and a bridge outside
serves.

## A route

Each of a proxy's `routes` (`link_sandbox_route::Route`) is a way out
beside the network, `{"name", "prefix", "needs_url_secret"?, "ports"?,
"bridge": {"socket", "start"?}, "given", "env"?}`.

### Where it's mounted

The proxy mounts it at `/_link/<prefix>` (behind `/_link/url/<secret>/`
with `needs_url_secret`), and hands each call to it, with `given`, to
[the bridge at `socket`](#routes-bridges), started by `start` when nothing serves it there. `env` is what the program is told
of it, `{proxy}` in each value its address.

### `caller`

`caller` (`link_sandbox_bridge::Binding`, `{"identity", "key"}`) is who calls
the routes, which the proxy proves on each call. With routes and none,
`wrap` binds one for the run, `run-<name>` in this machine's keyring,
which the proxy unbinds as it ends.

### Placeholders

The program reaches the hosted model at
`link_model_contract::models::route::url` with the key's `placeholder`,
a local model at `local_url`, and a secret's service at
`link_secret_contract::secrets::route::url` with the secret's
`placeholder`. Each is made by `link_sandbox_route::Placeholders`,
random for each run and holder, and refused by the route's bridge unless
it's one; a secret's `Sealed` says where it may be sent and what is
refused.

### `model`

A hosted model's key (`link_model_contract::models::route::hosted`):
`given` is `{"provider", "key": "model" | "fallback", "placeholder",
"server_tools"?}`, naming the caller's key, never holding it.

### `secret`

Secrets each added to its own service's calls
(`link_secret_contract::secrets::route::route`): `given` is
`{"granted"?: [{"name", "kept": {"own": <use>} | {"variable": <name>} |
{"token": {"grants"}}, "placeholder", <rule>}], "secrets"?: [{"name",
"value", "placeholder", <rule>}]}`.

### Its rules

Each rule is `"host", "paths": [{"prefix", "rest"}], "header"?:
{"name", "value", "basic"?}, "refuse"?, "methods"?, "public"?`.

### `granted` and `secrets`

`granted` names a secret the store grants the caller, and `secrets`
hands a run's with its value, never in a file: a routes file holding one
is refused, written or read.

### Kept by the keeper

A keeper-backed `secret` route replaces `given` with an opaque HPKE
grant before proxy handoff, so proxy configuration holds ciphertext for
this route. The keeper's secret bridge opens it only for the bound
socket UID, agent, owner, proof and current grant. Root binders can name
the helper UID; ordinary callers can name only their own.

### `local-model`, `mcp` and `open`

`local-model` is this machine's runtimes at `ports`, inference only
(`local`). `mcp` is tools that run outside (`link_sandbox_client::routes::tools`,
`{"tools", "desktop"?}`), and `open` pages opened in your browser,
asking you (`open`, `{"hosts", "desktop"?}`, `["*"]` any).

## Routes' bridges

`lnk-model bridge`, `lnk-secret bridge` and `lnk-sandbox bridge`,
each `--socket <socket> --keys <keyring> --settings
<dir> --runtime <runtime>` (hidden): a route's bridge
(`link_sandbox_route::bridge`), one a module for each user of a machine,
at `<runtime>/sandbox/bridges/<module>.sock`.
Read by `lnk sandbox proxy`.

### How one starts

The first proxy whose route needs it starts it (`bridge.start`) with
the system's `PATH`, `<dir>` as `LNK_INSTALLATION_DIR` and its home folder
as `HOME`. Where systemd runs Link's services and a user manager is
reached, that's a transient user unit, `lnk-bridge-<module>` (`systemd-run
--user --collect`), its stderr appended to `<module>.log` beside its
socket, and elsewhere a process group of its own. A lock keeps it to
one. It exits once idle, once its socket isn't its own, or once an
upgrade replaced its program and no call is in flight.

### A call

It serves `link-sandbox-bridge`'s stream call, each proved as an agent bound in
`<keyring>`. The action is `link_sandbox_route::Call` (`{"route",
"sandbox", "upstream", "listed"?, "given", "to": {"request": {"head"}} |
{"port": {"port"}}, "inbound"?}`): the route's name, the sandbox's,
where its traffic leaves from, whether its call's host is one of the
sandbox's own, what the route is given, and the request's head, or a
port of the route's to reach. Then comes the request's body as it
comes.

### Its verdict

The bridge answers one JSON line, `link_sandbox_route::Verdict`
(`{"to"?, "outcome": "allowed" | "refused" | "failed"}`, where it goes,
never a path, which the proxy logs), or `link-sandbox-bridge`'s error envelope,
then the answer, as it comes.

### `model` and `secret`

Both read a key or secret the route names from the store at each call
(`link_secret_contract::for_agent`, of the caller's service), refusing one
the store doesn't grant the caller, or whose placeholder wasn't made for
its value. `secret` adds each secret to its own service's calls by its
rule, refusing the rest.

### Agent tokens

An agent token may carry an `assertion` (`{url, proof}`) beside its
`kept` selector. The trusted bridge trades that agent-specific proof at
the issuer with `{resource, grants}` and adds its returned access token
outside the sandbox. It trades at every call, follows no redirect and
refuses an expired token or failed exchange. Such routes are handed
directly to the proxy and cannot be saved as names alone in a file.

### `model`'s answers

`model` adds the hosted key to a request showing its placeholder, its
provider's tools that run on its servers removed, and adds each
answer's token counts to the caller's mapped `Hidden::MeasureTokens`
folder (`~/.lnk/safe/measures/credentials` by default), `<agent
ID>.json` (600, `link_measure_contract::client::Tokens`,
`link_measure_contract::client::tokens`, under a lock), which `lnk
measure` reads as the agent's.

### `local-model` and `sandbox`

`local-model` sends inference alone to `ports`; `sandbox` serves `mcp`
(each tool, asking you where its calls aren't allowed) and `open`,
asking you before each page.

### What `mcp` and `open` reach now

`given` names what a call may reach as its proxy started; the bridge
never trusts it alone. At each call it asks the modules that keep agents
`sandbox.grants` (`link_sandbox_client::grants`, `{"agent", "sandbox"}`,
answered `{"agent": {"tools", "browser"?}}` or `"not_mine"`) what the
agent the call is proved as may reach now, and a call reaches only what
both say. A tool or a page's host taken back since is refused. A
person's own run, bound as `run-<name>`, reaches what its routes say.

### A keeper that doesn't answer

An agent no module answers for, or whose keeper fails to answer,
reaches nothing. Its `mcp` call is refused with that reason in the
keeper's own words, such as a module built from other sources than the
bridge, and the steps that help: [`lnk upgrade`](../../../../../../../../../../docs/more/modules/guide.md#upgrade) for a module from
another build, [`lnk agent restart`](../../../../../../../../../agents/agent/docs/agents/start/stop.md#stop-and-restart) for a keeper that didn't answer.
Only a tool its keeper didn't grant is refused as one the sandbox isn't
granted now.

### A keeper that isn't a module

A service that binds agents into sandboxes, where no module keeps them,
writes what each may reach as it binds it (`grants::keep`): the same
`{"tools", "browser"?}`, in `state/sandbox/grants/<agent>.json` of the
settings its bridges serve, and takes it back (`grants::forget`) as it
lets the agent go. The bridge reads that file only when no module keeps
the agent, and never when a module failed to answer.

## Models and Secrets

[Models and Secrets](models-and-secrets.md) explains placeholder replacement,
credential handling, and the bridge's responsibility when a route reaches a model
or a service that requires a secret.
