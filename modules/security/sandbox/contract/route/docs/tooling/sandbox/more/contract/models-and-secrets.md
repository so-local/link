# Models and Secrets on the Wire

How a hosted model's key and a service's secret are added to a
sandbox's calls outside it, by [a route of its proxy's](routes.md).

## Routes, served outside

A model, a secret's service, a tool and a page to open are each a route
of the proxy's: a prefix of `/_link/` it mounts, and a bridge outside
every sandbox that serves it, of the module whose it is (`lnk-model` for
models, `lnk-secret` for secrets, `lnk-sandbox` for tools and pages).
The proxy names none of them.

### Asking for one

A program asks for one by its path (`GET /_link/...`), or, sending every
request through its proxy, at the proxy's own address (`GET
http://127.0.0.1:<port>/_link/...`, with the run's proxy secret), the
same call, and a page's refused as by its path.

### Handed to its bridge

For a call to a route the proxy checks the URL secret, when the route
needs one, and hands the call to the route's bridge on a Unix socket,
proving each call as the agent it was bound to call as, with that
agent's key (a run with no agent is `run-<name>`, bound for the run
alone): the request's head, what the route is given (a key, secrets,
tools), then the body as it comes.

### Its verdict

The bridge says its verdict before anything else, which the wire's log
keeps, then answers; a call whose program hangs up is cut off at the
bridge too.

### One bridge a module, for each user

Its socket is in Link's settings (`bridges/<module>.sock`), which no
sandbox sees. The first proxy that needs it starts it, as the proxy's
user, with an environment of its own (the system's `PATH`, and the
settings' `HOME`), never the program's; a lock keeps it to one, and it
exits once no call has come for a while, or when its socket is no longer
its own.

### Only proved calls, and nothing held

Every call is proved against the machine's keyring before its action is
read, [as every bridge's is](../../../../../../../bridge/docs/tooling/sandbox/more/bridges.md#bridges-link-sandbox-bridge):
another program on the machine, or an agent once stopped, gets nothing.
A key or secret is given with each call, from the bind the proxy holds,
and the bridge keeps it no longer than the call.

## On the loopback alone

A sandbox on the loopback alone (a harness's health check) reaches the
ports of this machine it names, on its loopback, and nothing else: no
proxy, so no key and no wire, no internet and no other port.

### On each OS

On Linux it is a network namespace of its own, the ports relayed in
through a socket in the folder `loopback` names (700), by a relay that
checks each port outside and carries nothing else. On macOS it is
Seatbelt's rules allowing those ports alone.

### One kind of network

A confinement's network is one kind of `link_sandbox_client::Net`
(closed, the loopback, its proxy, other sandboxes' proxies, or the
host's), so the loopback alone beside a proxy, the internet, `[lan]` or
`[browser]` can't be written.

## A local model's port

A model runtime's port (`--model`, an agent's local model) is filtered
there, by the proxy's `local-model` route (the models module's bridge).

- Each request's head is read, and only these pass, by method and exact
  path: `/`, `/health`, Ollama's chat, generate, embed, tags, ps and
  version, and `/v1`'s chat, completions, embeddings and transcriptions.
  The route answers `/v1/models` (plain ids) and `/_link/model/show` (one
  shape) itself. A body up to 32 MB goes with its head; a longer or
  chunked one streams.
- A runtime that can't be reached gets the client a 502 saying why, not
  a connection closed with nothing.
- Anything else gets a 403 and never reaches the runtime: pull, create,
  copy, push, delete, an encoded or relative path.
- A request for a model's answer (Ollama's chat and generate, `/v1`'s
  chat and completions) asks for it uncompressed, and its usage is
  counted as it passes, as for a hosted model.

### One request a connection

Each is rewritten to close its connection after its response, so the
next one is read too, with no protocol switch and one way to tell its
body's length.

### `Connection: close`

Its response says `Connection: close` in place of what the runtime
said, so a client that keeps connections opens a new one for its next
request rather than sending it on one that's closing (a runtime that
keeps its own, as Ollama's does, answers as if it stays open).

### On the proxy itself

The same filter answers on the proxy at `/_link/url/<secret>/here/<port>`
(`$LNK_LOCAL_MODEL_<PORT>`), where the secret must be the proxy's and
the port one of the specification's model ports, else a 403.

### On macOS

That's the only way to the runtime: Seatbelt can't filter a port, so the
model's port is closed to the sandbox. An agent's harness is set up with
the proxy's address, with `Host` set to the runtime's. Its proxy is on a
loopback port fixed per agent, the last of its block: `main`'s is
`40781`.

## A hosted model's key, added by the proxy

An agent's sandbox has this with keys on the wire: by default for an
agent whose harness takes the proxy's address (Link Harness, and a
harness whose adapter says so), any agent's with `lnk agent secret wire`.
[`lnk sandbox run`](../../../../../../../docs/tooling/sandbox/guide.md#sandbox-guide) adds no hosted model's key. Providers: Anthropic,
OpenAI and OpenRouter, at their own addresses or at one of a model's own
(a module's declared provider's, a `base_url`), on a route of its own.

### Its placeholder

The program reaches `http://127.0.0.1:<its proxy's port>/_link/model`
with a placeholder key: the key's prefix (`sk-ant-api03-`, ...),
`link-wire-` and a tag, none of the key itself.

### The tag

An HMAC-SHA256, under a secret random for each run and holder, of what
the placeholder stands for: never a hash of the key, so it says nothing
of it, and one read from a harness's home is no use to another agent, or
after the agent's next start, which makes the secret anew.

### Who makes it

Whoever makes the confinement makes the placeholder and gives it to the
proxy's `model` route beside the key; a proxy kept across a start
without being given the new one refuses the harness's calls, failing
closed.

### What its bridge does

The route's bridge (the models module's) is handed each call, and:

- refuses a request without that placeholder as its key, so another
  program on the machine gets no key added;
- reads each request: a head of at most 16 KiB, a body with a length
  (never chunked), at most 32 MB, arriving within 2 minutes, and JSON;
- passes only inference, by exact method and path: Anthropic's
  messages, token counts and models; OpenAI's chat completions,
  responses, embeddings and models; OpenRouter's the same and its
  completions, under `/api`;
- drops whatever credentials, `Host` and hop-by-hop headers the program
  sent;
- adds the real key and speaks TLS to the provider, checking its
  certificate against the machine's trust store;
- answers one request per connection from the program, and keeps its
  own connection to the provider open for the next.

## Hosted Model Requests

Hosted requests can use server-side tools and fetch URLs. The bridge filters
those operations before it forwards a request carrying the model credential.

### Server-side tools

It removes the provider's server-side tools by allow-list, with MCP
servers, containers and OpenRouter's web modules, `:online` on a model
or any of its fallbacks (`models`), and refuses OpenRouter's presets
(`@preset/...`), which can bring their own.

### Searching by nature

With a list of hosts, it also refuses a model that searches by nature,
or a fallback that does (Perplexity's, OpenAI's search and deep research
models).

### What stays

A function or custom tool, Anthropic's bash, text editor, computer and
memory, and OpenAI's local shell and patch tools stay, and Link turns
none of the rest back on.

### A URL to fetch

It refuses a request asking the provider to fetch a URL: a web address,
in any case, under any key ending in `url` or `_data` (`image_url`,
OpenRouter's `file_data`), in any block. Only the `input` of a tool's
call (`tool_use`, `function_call` and the like) is left out, never the
request's own.

### Its usage

For a request for a model's answer (messages, chat completions,
completions, responses), it asks for it uncompressed (`Accept-Encoding:
identity`), and reads the usage the answer gives as it passes it on,
unchanged and never held back: the numbers of its tokens, added to the
agent's count for [`lnk measure`](../../../../../../../../../tooling/measure/docs/tooling/measure/README.md#measure) (`~/.lnk/safe/measures/credentials/<agent
ID>.json`, 600), never a word of the answer.

### The key itself

The route names the key, the caller's model's or fallback's: the models
module's bridge reads it at each call from your secrets granted the
caller (`routes.json` holds none), refusing a caller granted none. It is
never in the sandbox, its environment or a command line.

### Its provider

The provider is dialed like any host: never this machine, through the
route's SOCKS proxy if there is one, whatever hosts the specification lists.

## A service's secret, added by the proxy

An agent's sandbox has this with keys on the wire, for a Telegram bot's
token when its harness speaks Telegram itself: of Link's connection
modules, Telegram's is the one that gives a rule. The sandbox knows no
service.

### Its rule

Each secret comes with its rule, data the module owning the service
gives, as `lnk-telegram rule --json` does by its [contract](../../../../../../../../../agents/connections/contract/docs/agents/connections/more/protocol/README.md#the-contract):
one host, the paths a call may take, where the secret goes (in a path,
or a header), and the paths refused.

### Its placeholder

The harness is set up with `http://127.0.0.1:<its proxy's
port>/_link/secret/<name>` as the service's root, and in place of the
secret a placeholder: `link-wire-` and a tag made as a model credential's is,
random for each run and holder, after the part the rule says isn't
secret (a bot's id, digits only).

### What its bridge does

The proxy hands each call to its `secret` route, whose bridge (the
secret module's):

- refuses a call without that placeholder in its path or header, so
  another program on the machine can't use the secret;
- reads each request's head, as for any request, and passes only `GET`
  and `POST` (or the rule's methods) of the rule's paths, taken as sent:
  a name of letters and digits (Telegram's `/bot<token>/<method>`), or a
  path of plain parts (`/file/bot<token>/<path>`), no encoded, relative
  or empty part;
- refuses the rule's refused paths, whatever their case (Telegram's
  `setWebhook`, `logOut` and `close`, which would hand the bot's
  updates, or the bot, elsewhere);
- puts the real secret in the path or header, drops whatever `Host`,
  `Authorization` and hop-by-hop headers the program sent, closes the
  program's connection after its answer, and passes the body as it
  comes, long polls and uploads too.

### Its host

It speaks TLS to the rule's one host (a name, never a wildcard or an
address), checking its certificate against the machine's trust store,
and keeps that connection open for the next call when the request's body
has one length. A chunked upload is the last call on its connection. The
host is dialed like any host, whatever hosts the specification lists.

### Its routes, never written

An agent's proxy, which its service starts again, reads its routes from
`~/.lnk/safe/sandboxes/<name>.routes.json` (600), which names each secret and
holds none: the bridge reads it from the store, granted the caller, at
each call. A secret is never in the sandbox, its environment or a
command line.

### Handed on a pipe

`lnk sandbox run` and [`lnk agent harness run`](../../../../../../../../../agents/agent/docs/agents/harnesses/use/harness/README.md#inspect-the-sandbox) start their proxy at once
and hand it its routes on a pipe (`--routes-handed`), as Link hands its
own programs their secrets: never written to the disk, and the pipe
closed once read, before anything runs.

### Every connection's rule first

The harness starts an agent only once every connection's module has said
whether it has a rule: a connection whose module can't say never hands
its secret to the harness.

## Connections kept to a provider and to a secret's service

The bridges keep their connections to a hosted model's provider and to
a secret's service (Telegram's API) open between calls, so a call
doesn't pay for DNS, TCP and TLS each time.

- Up to 4 per host and way out (where the traffic leaves from), for 30
  seconds, in the bridge of one user, so never another user's.
- Only once the answer was read to the end its head gave, with nothing
  after it; any other is closed.
- Only for a request whose end the bridge wrote itself: a hosted
  model's body, which it sends with its own length, or a secret's call
  whose one `Content-Length` it copies exactly. A request with a
  `Transfer-Encoding`, or a length said twice, closes its connection.

### Sent again

A model request on a kept connection that the provider closed with
nothing back is sent again, on a new connection, only when it wasn't
written, or the connection closed within half a second of it: the
provider was closing it as it was taken. One closed later may have been
run, and is answered as a failure, never sent twice.
