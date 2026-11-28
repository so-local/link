# Security

The model module limits what callers can reach through a served runtime and what discovery exposes about this machine. Hosted typed actions use the proved carrier described below to reach their provider.

## Only the model API is served

`lnk model serve` forwards
only the runtime's OpenAI-compatible API.

- Only the inference API is forwarded, with `GET`, `POST` and `OPTIONS`:
  `/v1/chat/completions`, `/v1/completions`, `/v1/embeddings`,
  `/v1/responses`, `/v1/audio/transcriptions`, `/v1/models`, and
  reading one model at `/v1/models/<id>`. Another method on these paths
  gets 405.
- `GET /` shows how to call it; any other path is a 404, so a runtime's
  own endpoints under `/v1` (Jan's model pulls, LocalAI's file uploads,
  vLLM's adapters) aren't reached.
- Paths with `.` or `..` segments, `%` or `\` are refused, so a visitor
  can't reach management endpoints such as Ollama's `/api/pull`,
  `/api/create` or `/api/delete`.
- When the runtime can't be reached, visitors get a fixed 502 message.
  The details go to stderr.
- Nothing is logged per request: no prompts or completions.

### `--model`

`--model` pins every request to one model and makes `/v1/models` list
only it. The body must then be a JSON object of at most 16 MiB (413).
Such bodies hold at most 64 MiB at once.

### Room for bodies

Each body takes its whole length when it starts (its `Content-Length`,
or 16 MiB without one), so bodies half sent never wait on each other
for room, and holds it until the copy with the model set in it is
written to the runtime, so that copy counts too, and a runtime slow to
answer holds none of it.

### Waits

A body that waits for room for 30 s gets 503; one that sends nothing
for 30 s gets 408, and so does one still arriving after 10 minutes.

## On loopback only

`lnk model serve` listens on a port on 127.0.0.1, and answers only a
request naming this machine (`Host` `127.0.0.1`, `localhost` or
`[::1]`), so a web page that points its own name here (DNS rebinding)
gets a 403.

### Other local users

Other local users can reach that port while it runs, as they can reach
the runtime's own port.

## Discovery

[`lnk model list`](local.md#see-whats-running), and serving without `--upstream`, send `GET /v1/models`
to the runtimes' default ports on localhost, and to the addresses added
with `lnk model runtime add`.

- An added address is always on this machine: `localhost`, `127.0.0.1`
  or `[::1]`, with no path, query or password. `add` refuses any other,
  and discovery skips one edited into `~/Link/Safe/Models/models.toml` (600).
- `~/.lnk/safe/models/facts.toml`, the models you describe, is read
  strictly: a key Link doesn't know, a window under 1024 tokens or a
  negative price is refused, naming the line, and at most 1 MiB is
  read.
- Link's own facts, built into `lnk`, are refreshed from OpenRouter's
  public models list at each release, bound by the file in the
  repository: a window or longest answer more than halved, a price
  moved more than tenfold, or a model listed under several ids, none
  its own, keeps the reviewed figures.
- An agent, like the tunnel, takes an added runtime's models as it takes
  Ollama's, so add only a server you trust with the agent's prompts.
- The requests go through no proxy, and read at most 1 MiB back.
- A local app on one of those ports that happens to answer with a model
  list is treated as a runtime. `lnk` prints which address it exposes.

## Installing Ollama

`lnk model install`, and the local model [`lnk agent start`](../../../../agent/docs/agents/README.md#start-your-agent) offers,
install Ollama when it's missing.

- It asks first, unless `--yes`. `lnk agent start` passes `--yes` once
  you picked the local model.
- On macOS, Link downloads Ollama's app from `ollama.com` over HTTPS and
  installs it only if Gatekeeper accepts it (`spctl --assess`: signed by
  its developer and notarized by Apple).
- On Linux, it runs Ollama's own install script from `ollama.com`, which
  asks for sudo and installs Ollama as a system service. That script is
  Ollama's to trust, as when you run it yourself. It's saved to a folder
  made new for it (as `mkdtemp`), only yours, so another user of the
  machine can't swap it before it runs.
- Models come from Ollama's library through Ollama's own API, and
  `lnk model delete` deletes them the same way.

## Typed action grants

The namespaced model routes bind a proved caller to a hosted provider
or local port. Only the grant supplies the backend and key; requests
can choose a plain model name only within its allow list. The bridge
rechecks a named hosted credential at every call. Each registry route
has a time and answer budget, and unknown input fields are refused.
Link's provider mappings bound their reads before parsing. Redirects
are refused. Native protocol carriers remain for outside harnesses.

### Refusals and errors

A refusal says why in Link's own words when the caller's request is the
reason (a window or answer past the grant, a model outside its allow
list). A provider's refusal gives its status and the message of its
JSON error, at most 300 characters, with the credential sent taken out.
Every other error exposes only a fixed message, never a provider URL,
host path, or credential.

### Reused transports

Loopback actions share an HTTP transport without default authorization or
cached responses. Each request still resolves its current grant and model.
Metadata lookups also reuse their transport while reading fresh answers.
These clients use plain HTTP without loading TLS trust roots; a hosted
action's separate client is bound only to its own proved Unix carrier.

### Approved exits

Hosted typed actions use the same native carrier as outside harnesses,
through a private, temporary Unix socket. The carrier adds the credential,
filters server tools and dials through the proved call's approved exit.
The socket is private from creation and removed when the action ends;
its connections end with it. Provider addresses never come from inputs.

### Context records

The bridge keeps each model call's census as names and numbers only:
part paths, sizes, token counts and the model's name, never a word of a
prompt or an answer. A tool's name that isn't plain is kept as `other`.
The records are in a folder only this user can read, a file for each
chat, mode 600. A caller reads only [its own agent's records](context.md): the agent
is the proved caller's, never one a request names.
