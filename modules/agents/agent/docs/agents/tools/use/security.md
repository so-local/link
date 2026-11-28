# Security

What Memory, Web, Browser, Run, File, and Desktop can reach, and how their work stays within the agent's grants.

## Read Identity

The host establishes a managed reader's identity separately from a
service's statement about content.

### Configuration Checks

At configuration, Link checks a managed File or Link Memory definition
against its native program and recorded command fingerprint. A configured
namespace must also target that native reader. The private handoff binds
the source's owner, agent, and operation to its server endpoint. The
harness resolves that endpoint before accepting managed read provenance.
Custom, changed, missing, or duplicate matching definitions stay unverified.
Neither model arguments nor a service's receipt can grant this authority.

### Retained Evidence

Execution evidence retains the identity and fingerprint, never the private
endpoint or authorization token. The identity describes the setup checked
at configuration, while content receipts and delivery limits describe reads.

## Memory

Where an agent's memory runs, and what it reads and writes.

- The engine runs outside the agent's sandbox, as its tool (`lnk sandbox
  tool add`, as `memory-<agent id>`), and in a sandbox of its own.
- The engine's sandbox reads the agent's Home, conversations, skills and,
  of `~/.lnk/exec/bin`, only its own program, those of the Link modules
  it names (Link Document Converter's), and those of the store drivers
  that keep the agent's conversations, each where installed.
- It writes only its
  folder in the agent's own (`agent/memory/<engine>`) and its
  `agent/memory/`, for the agent's notes, unless the agent's sandbox
  file locks its instructions (`instructions` under `[files] read`).
- Never `chats/` or anything else of either folder: a test
  tries. With the agent's sandbox off (`--no-sandbox`), it runs
  unconfined, a process each session. A session ends with its input
  closed, or its process killed: on Linux the engine is sent SIGTERM
  then too, so one that ignores its input doesn't outlive the session.
  On a Mac, an engine that ignores its input outlives a killed session.
- Link Memory serves all of one owner's sandboxed agents on a machine
  from one process, in one sandbox holding what each agent's would hold:
  every one of their Homes and own folders' conversations and skills
  read, every state and notes folder written. So that process can read
  each of those agents' files.
- It answers each session from the one agent Link named as the session
  opened, in a line the agent never writes: the tool Link defined for it
  says which. Every session after is that agent's alone, its search,
  reads, list and notes. A test shows one agent never finds, reads or
  notes another's, and a session naming an agent it doesn't serve is
  refused. Two owners' agents never share one.
- The service's socket and what it serves are in
  `~/.lnk/data/agents/memory/<owner>`, which every sandbox hides, so only
  the service and what runs outside any sandbox reach it, and it takes a
  session only from a process running as you, as the system names the
  one at the other end: another user's is refused.
- Of that folder the service's sandbox writes only its working folder
  (`run`, its socket's): the records of what it serves, what it's handed
  of them and its log, which Link reads and opens outside any sandbox,
  it can only read, so the engine can't change what its next start is
  confined to or point its log elsewhere.
- Its engine's install runs in the same sandbox with no network: the
  engine came as a signed module, so the install fetches nothing, and
  memory installs on a cloud's machine too.
- It starts at a session's first need and ends a few seconds after its
  last, so an idle owner runs nothing of it.
- Its sandbox's network is all its agents' together: the service
  reaches the port of every model it serves for them, and one agent
  whose engine finds this machine's models itself, with none served for
  it, gives the service `[network] mode = "host"` for every agent.
- Its embedding model is served for it outside its sandbox (`lnk model
  serve`, once for the service, stopped with it), and its sandbox is
  given the address in `$LNK_MEMORY_MODEL`, a port of this machine's
  loopback chosen before it starts.
- Every model of the service is started at once and never waited for:
  its engine starts at once with each model's address, and searches by
  words until the model answers there.
- A model that's slow or fails keeps its address and is started again
  every 30 s while the service runs; one no runtime here has isn't.
  Link Memory follows no redirect from it, and reads at most 64 MiB of
  an answer.
- To reach it on Linux, Link Memory's sandbox is given that port alone,
  through its proxy (`[localhost.linux] models`), at the model's own
  address. Only inference calls pass, and it reaches no other port and
  no internet, so memory searches by meaning on a cloud's machine too.
- On macOS its sandbox has `[network] mode = "host"`
  (`[network.macos]`), the way a sandbox there keeps a model's address.
  Nothing it indexes leaves this machine: the model runs here.
- The model's server takes no key: while it runs, any program of this
  machine can call it on its port.
- Link Memory indexes only Home's `memory/` and the agent's
  conversations: the rest of Home it never reads, and of the agent's
  own folder it reads only its conversations and skills.
- It extracts the text of PDFs and Word documents in `memory/` in a
  child process, never in the one holding agents' keys: Link Document
  Converter (`lnk-document-converter`, the `document-converter` module),
  the document on its stdin and the text on its stdout. Without that
  module, such a document is indexed by its name alone.
- The converter is given no
  environment and none of its secrets, at most 30 seconds, 30 seconds on
  the processor and 1 GiB of memory (on a Mac, where memory isn't
  limited, the 30 seconds alone), and no core dump.
- At most 32 MiB a file and the first 2 MiB of its text. A parser that
  fails, crashes or runs past its limits fails that file alone, found
  by its name.
- Memory's service, which serves many agents, runs the converter in
  [`lnk sandbox run`](../../../../../../security/sandbox/docs/tooling/sandbox/guide.md#sandbox-guide) with nothing granted, so a crafted PDF that takes
  over the parser reaches no network and no one's files. A single
  agent's memory already runs inside that agent's sandbox. With the
  agent's sandbox off, the converter reaches what its user can.
- Where an agent's machine keeps its conversations, its notes or its
  memory off its folder (`~/.lnk/safe/agents/policy.toml`'s `[storage]`: in SQLite, a
  bucket, or a box's store), or its files folder in a bucket on a
  machine that runs agents only while they answer, memory's service
  binds the agent:
- Link hands it the agent's key for this start and where its bridges
  are, and memory reads `memory/` through the files bridge and its
  conversations, notes and index's copy through the storage bridge, as
  that agent, proving each call. It drops the key when the agent sleeps,
  and a session still open answers nothing more.
- Its socket takes a session only from a process of the user it runs as,
  as the system says (`SO_PEERCRED`, `getpeereid`), or of the one other
  user it's told (`--peer`). Anyone else's session is refused, and told so.
- On a computer, that other user is the one who started it: on a Link
  box, its controller's sandboxed work runs as a fresh worker user, so
  the service is that worker while the agents it serves are the controller.
- The service binding a machine's agents, run as a user of its own, names
  their user, its socket then shared with that user's group alone
  (`--socket-group`, mode 660) in a folder no one else enters.
- A session's first line, naming its agent, or a control for one,
  carries no proof: who may connect is all that keeps anyone else from
  naming an agent bound there. Every binding and control names the agent
  by its owner and ID, so one owner's agent is never served or ended as
  another's of the same ID.
- On a computer it reaches the storage bridge at a socket of its own, in
  a folder of its owner's memory service that its sandbox reads and
  every agent's sandbox hides; through it, it changes besides what the
  agent's sandbox writes only its `memory` collection and, where its
  notes are written, `memory/notes.md`.
- Its cache of each agent's index is plain SQLite on the machine's disk,
  mode 0600 under its own user, never in a sandbox; the copy of it, kept
  with the agent's memory or beside its files folder, is encrypted where
  the place encrypts (a bucket always does), and a write back is made
  only from the version the cache was read from, so one machine's never
  overwrites another's unseen.
- A cached copy of an index is the owner's to change, and this process holds every bound
  agent's key, so a copy fetched is checked whole and for memory's own
  schema alone before it's used, and made again otherwise; every index
  is opened in SQLite's defensive mode, its schema untrusted.
- The bridges a binding names must be the machine's own (the ones its
  service is told of, or the agent's own storage bridge socket): any
  other is refused before the key goes anywhere. A binding is served,
  even from a cache already there, only once one of them takes its key,
  and a cache is used only while it's of the copy there now.
- Link Memory reads only its folder: it never follows a symbolic link or
  goes into a hidden folder, and `read` serves only what it indexed. It
  opens every name from its folder's handle without following a link, so
  a link swapped in while it reads is refused too, and reads only
  regular files, at most 1 MiB each, however they change meanwhile.
  Tests show nothing outside is reached.
- Its reading holds by path: a hard link in the folder is a regular file, read
  like any other, so a hard link a harness makes there to a file it can
  already read is indexed.
- Its four calls that read never ask you first, nor does `remember`,
  which writes only the notes file Link names, `memory/notes.md`, at
  most `[tool.memory] notes` (4 KB), never through a link. What the
  agent notes is in its prompt at every message after, as its standing
  instructions are, so a note it was talked into stays until it, or you,
  change it:
- `lnk agent show --instructions` shows it, and the file is yours to edit.
  Another engine's calls that write ask, as any tool's do.
- The agent reaches it only through its proxy, which logs each call, which [`lnk sandbox log show`](../../../../../../security/sandbox/docs/tooling/sandbox/security/log/README.md#see-every-call-on-the-wire) shows. Each agent has its own, over its own files; one
  agent's proxy names no other's.
- Its index holds a copy of what it indexed, and each chunk's vector,
  in a folder only you can read (700), which no harness reaches. It
  also keeps the path and title of each document deleted from the
  folder, the latest 10,000, for `list {since}`; your agent can list
  them, as it could the documents before. [`lnk agent memory stop`](../../../../../tools/memory-tool/docs/agents/tools/memory/guide.md#stop-memory) leaves
  it; delete it by hand.

## Web

Where Link Web runs, and what it reaches.

- Link Web runs outside the agent's sandbox, as a tool you add (`lnk
  sandbox tool add web`), with your user's rights and the environment
  you gave it. The agent reaches it only through its proxy, which logs
  each call (`lnk sandbox log show`), and only once you allow it (`lnk agent
  permission add --tools.names web`).
- A fetch never reaches this machine or its networks: loopback, private
  and link-local ranges, carrier-grade NAT, a cloud's metadata service
  (Azure's host, `168.63.129.16`, too), this machine's own addresses,
  and the same written as IPv6, by the sandbox proxy's own rule.
- Every address a name resolves to is checked, one forbidden refuses it
  whole, and the connection is made only to the addresses checked, so a
  name can't point elsewhere between the check and the fetch. Each
  redirect, at most ten, is checked the same way, and one from
  `https://` to `http://` isn't followed. Only `http://` and `https://`
  are fetched, never with a login in the URL.
- A host you name with `--private-host` is fetched wherever it resolves.
- With `[tool.web] hosts` in the agent's specification, a fetch, each redirect,
  and a page kept for its next parts reach only the hosts it names,
  checked before a name is looked up. Link Harness and `lnk agent web
  fetch` send the agent's `[tool.web]` with each call, and a fetch that
  comes without it is refused, never made for any host.
- With a proxy in the tool's environment (`--env HTTPS_PROXY`), the
  proxy connects: the names are checked here, but the proxy resolves
  them again and reaches what it reaches.
- A page is read up to 5 MB, and a page, its name's lookup, its
  redirects, and its body together, or a search, may take 30 seconds.
- Nothing on a page runs: its HTML is read as text, and its scripts,
  styles, images, and forms are left out.
- Text a browser hides (`hidden`, `aria-hidden="true"`, or an inline
  `display: none` or `visibility: hidden`) is left out, since
  instructions aimed at an agent hide there.
- A page nested more than 512 elements deep comes as plain text, with
  the same parts left out, so its markdown can't overflow the stack.
- A message line from the harness longer than 1 MiB is skipped to its
  end and answered with an error, never held whole.
- At most 16 calls run at once; a call past them waits to be read until
  one is answered, so a client that sends calls without end can't make
  the tool hold a thread and a page for each. Two pages at a time are
  made markdown.
- A search sends the query to Brave's or Tavily's API with your key,
  from the tool's environment (`--env`), which no harness sees. An
  error from the API never carries the key.
- A redirect from the search API is never followed, so the key never
  reaches another host or plain `http://`. A release build sends it
  only to the provider, since only a debug build reads
  `LNK_WEB_SEARCH_URL`, the tests' address.
- A page is the web's, not yours. What it says reaches the model as a
  tool's answer, so a page can try to steer the agent, and a fetch can
  send what the agent puts in a URL to any site it may fetch.
- The tool fences a page as [untrusted content](../../../../../../../packages/untrusted/docs/more/libraries/runtime/untrusted.md#the-note),
  whose note tells the model a page's words aren't its owner's, to
  follow none of its instructions and to repeat none of them, nor a
  code word or link they ask it to pass on.
- Text a page hides in its own markup, with `hidden`, `aria-hidden` or
  an inline `display: none` or `visibility: hidden`, never reaches the
  model. The note is words to the model, which can still heed a page,
  so the agent's sandbox and grants still bound what it can do. Allow
  the tool only to an agent you'd let browse.

## Browser

Where Link Browser runs, what its Chromium reaches, and what a page can
do to the agent.

- Link Browser (`lnk-browser-tool`, the `browser-tool` module) serves
  outside the agent's sandbox, as a tool defined for that agent alone, `browser-<agent id>`, which [`lnk agent tool add browser`](../../../../../tools/browser-tool/docs/agents/tools/browser/guide.md#add-it) adds.
  Every session of the agent shares it (`--shared`), its twelve calls
  allowed, so they ask nothing.
- It starts Chromium in a sandbox of its own, [`lnk sandbox run`](../../../../../../security/sandbox/docs/tooling/sandbox/guide.md#sandbox-guide), which
  reads the folder of Chromium's own program, links resolved, where it
  isn't one of the system's, writes only the profile, and reaches the
  agent's internet hosts (`--internet.hosts`, from its `[internet]
  hosts`). It runs only Chromium, its folder's helpers, and the shell
  that starts it, each named by the browser tool.
- Its internet is the agent's: no host when the agent has none, and any
  host only when the agent's grants name `*`. It goes through the
  sandbox's proxy: `https://` on port 443 and `http://` on port 80, no
  other port.
- This machine, your network and a cloud's metadata service stay
  closed, but for each host `--private-host` names (`--lan.hosts`, or
  `--localhost.ports` for `localhost:<port>`).
- A page that makes the proxy pause the browser's way out keeps it
  paused: the browser says so, and starts no other browser in its place
  until [`lnk sandbox start`](../../../../../../security/sandbox/docs/tooling/sandbox/guide.md#stop-a-sandbox).
- On Linux Chromium always runs with `--no-sandbox`, since its own
  sandbox needs a user namespace that Link's sandbox refuses inside; on
  a Mac it does so only as root. One page that takes over a renderer
  then holds all of the browser's sandbox, the profile's sign-ins
  included. The browser refuses a Chromium older than version 120 and
  runs it with `--disable-extensions`.
- Its profile, `~/.lnk/data/agents/<name>/browser`, keeps the agent's
  cookies and sign-ins, outside every harness's reach, in the profile
  rather than a system keychain (`--use-mock-keychain`,
  `--password-store=basic`).
- A sign-in is typed through `type`, so the model, its provider, and its
  logs see the password. Direct registration refuses the known personal
  profiles of the supported browsers, and refuses to start with no
  `HOME`.
- Each conversation has a page of its own, by the conversation and
  standing its call's `_meta` names, at most the agent's floor
  (`--floor`), so a page, what was typed into it, and `close` stay in
  their conversation.
- Only a turn standing as the owner opens in the profile's context, with
  its sign-ins. A teammate's, a member's or a stranger's opens in a
  browser context of its own, which sees none of them and keeps nothing.
- It opens only `http://` and `https://` pages, and refuses a URL naming
  `localhost`, a name under it, or an address of this machine or its
  network that `--private-host` doesn't name. The sandbox's proxy
  refuses the rest, however a link, a redirect, or a script asks.
- It downloads nothing and uploads no file, in every context. An alert
  is answered OK; a `confirm()`, a prompt or a question before leaving
  the page is dismissed unless the conversation's last `dialog` call
  said OK, and its words are said.
- The page is read in an isolated world, which the page's scripts can't
  see or change, and each note a page writes is capped at 2 KB and
  fenced. A click lands only where `elementFromPoint` finds the part.
- A page is the web's, not yours. Its text reaches the model as a tool's
  answer, so a page can try to steer the agent, and whatever the agent
  types into a page goes to that site. Add the browser only to an agent
  you'd let browse with its sign-ins.

## Run and file

Where `run` and `file` serve, and the sandbox each call runs in.

- Link Harness's `run` and `file` serve outside its sandbox, as its
  tools (`run-<agent id>`, `file-<agent id>`), and run their calls in
  sandboxes made from the agent's sandbox file by [`lnk sandbox run`](../../../../../../security/sandbox/docs/tooling/sandbox/guide.md#sandbox-guide):
  `run` one a call, ended with everything the call started; `file`,
  which runs only Link file, one a session. The servers only start
  those sandboxes, and read or run nothing of the agent's.
- Each call reads Home and the agent's instructions, skills and
  conversations in its own folder, and writes only the folders its
  `[files] write` names, so neither rewrites `chats/`, and by
  default neither changes its instructions or skills, which are
  yours. Every
  other agent's folder stays hidden, `--files.read '~'` or not, and Link's
  settings stay closed, as in every sandbox.
- Of `chats/`, `file` gives a call only its own conversation's folder,
  and a stranger's call none. Its server checks the conversation and
  standing the harness names (`_meta`). Its sandbox still reads every
  conversation, so a harness that names another conversation reaches it.
- `run` offers its call, a shell command (`sh -c`), only where its
  `[commands]` grants the shell (the `shell` toolchain, `sh`, or
  `allow = ["*"]`). The tool's own check is of the shell alone: what
  the command line names runs inside it.
- The sandbox holds every program the shell starts, and each one those
  start, to the same list, by the kernel. The shell's builtins (`cd`,
  `echo`, a redirection into a folder it writes) run as part of the
  shell itself, no program of their own.
- With `allow = ["*"]`, which Link's template has, it starts any
  program on its `PATH`.
- It reaches the internet only with `[internet]`, through a proxy of its
  own, holding to the file's `hosts`, `tcp` and `per_minute`, and its
  `[tools]`; never this machine's other servers or your network, but
  with `[network] mode = "host"`. Its calls ask nothing: `[commands]`
  allows or refuses.
- `file` refuses a path outside the files folder, a text file over 4
  MiB, an image or a PDF over 2 MiB, and any other file that isn't text.
- `file` reaches no network and runs only itself. It never follows a
  link. It opens each folder on the way from the one
  before, never through a link, and reads and writes the file through
  its folder's handle, so a link swapped in for a folder meanwhile is
  refused, never followed; `glob` and `grep` walk the same way.
- Where the files folder is kept in a bucket, `file` runs no sandbox and
  touches no disk: each call goes to the files bridge as the agent,
  proved with its binding's key, which reaches only its own files folder
  (no `teams/`), and holds to `[files] write` as well as the tool does.
- Through a files bridge, `write` uses this session’s last-seen version,
  or first reads the current version or absence for a new target.
  `edit` reads the current text and commits against that version.
  A concurrent target change before either conditional commit is refused.
- A `run` call starts in the files folder, opened the same way, and a
  folder swapped for a link meanwhile is never followed. A call's
  process group is stopped before its
  leader is reaped, so the group's ID can't be another's by then.
- A command runs at most `[tool.run] timeout`, a tenth less (2 s at least), or less where
  its call's `timeout` says, and what's
  kept of what it printed is `[tool.run] output`. Within that time it
  can use the CPU, memory and disk space its folders hold.
- A background command (`task__run` with `run_in_background`) runs in
  the same sandbox as any other `task__run` call, under a supervisor of its own outside it, for at
  most `[tool.run] background_timeout`, and at most `background` at
  once.
- Stopping it, or its supervisor dying however it dies, ends its whole
  process group: on Linux the sandbox dies with the supervisor, and a
  sweep kills what's left only when it's surely the command's, on the
  same boot, its group's leader the one recorded or gone. Stopping,
  restarting, moving or deleting the agent ends every one.
- Its output, `background/<id>.out` in the files folder, is written
  outside the sandbox through descriptors, never following a link, and
  kept to `[tool.run] background_output`. The agent's sandboxes read it,
  and write it only where `[files] write` names `background` or the whole
  files folder (`.`). Then any call can change any background command's
  output, but never a record.
- Its record, which names the process group, is in
  `~/.lnk/data/agents/<name>/run/`, outside the files folder, which every
  sandbox hides, so nothing the agent runs reaches it. Both
  go `background_keep` after it ended and was told, and when the agent
  is deleted or moves away.
- Its ending reaches the agent as a message from `run:<id>`, never as
  yours, at most once, through the agent's bridge's way in, a socket
  only you reach. Within its time a background command uses the CPU,
  memory and disk space its folders hold, as a call does. Each ending
  is a turn, so starts count toward a cap a day,
  `[tool.run] background_per_day` (100), beside `background` at once
  and the hops.
- A background command is its conversation's: `task__list`,
  `task__read` and `task__stop` reach only those the conversation that
  asks started, as its harness names it (`link.local/thread`), and
  answer nothing to a call that names none. Its output is still a file
  of the files folder, which every conversation's `file` reads.
- A start's conversation and hops come from the harness (`_meta`), as
  the scheduler's do, and are trusted as far: a harness that runs what
  the model writes could name another conversation or reset its hops,
  bounded then by `background_per_day` alone.
- With the agent's sandbox off, `run` and `file` aren't defined, since
  they run code only in a sandbox.

### Code

Where Link Harness's `code` serves, and the sandbox each language server
runs in.

- Link Harness's `code` serves outside its sandbox, as its tool
  (`code-<agent id>`), and runs each language server in a sandbox of its
  own (`lnk sandbox run`), made from the agent's sandbox file as `run`'s
  is but with no network.
- Each language server's sandbox reads Home and the agent's own folder's
  instructions, skills and conversations, your home only with `[files]
  read = ["~"]`, writes only the folders `[files] write` names, and
  starts only the programs of its `[commands]`, the server among them.
- A server runs the project's code (build scripts, macros), so `code`
  acts as the agent, as `run` does, and is never a stranger's.
- It starts only a server its `[commands]` allows and this machine has,
  and says which it would run otherwise. A server lives in a process
  group of its own, killed when the tool's session ends or the tool dies.
- The tool asks only about a file in the files folder, links resolved,
  and shows a line only of a file there: a place a server names
  elsewhere is given by its path alone, from `~` when it's in your home
  folder, so a server can't read out a file it was never shown. It never
  applies an edit a server proposes.
- The tool reads a file through its handle, opened folder by folder from
  the files folder's without following a link, so a link swapped in
  after a path was checked is refused rather than followed.
- What a server sends is bounded before the tool keeps it: each header
  line, message, diagnostics list, and piece of work in progress, and a
  reply no call waits on is dropped. An answer is bounded too, in places
  listed, characters of a line or message, and bytes.
- A call waits on its server at most `[tool.code] timeout`, a tenth less (2 s at least),
  and a server uses the CPU, memory and disk its folders hold, as `run` does.

## Desktop

Where Link desktop's display runs, and the door it is between the
agent's sandboxes.

### Its X server

Link desktop's server (`lnk-desktop-tool`) serves outside the agent's
sandbox, as its tool (`desktop-<agent id>`), shared by its sessions. The
display is `Xvfb`, an X server that draws to memory rather than a screen.
The X protocol is how a Linux program draws windows and receives mouse
and key input from a server it connects to.

### Xvfb's sandbox

The tool starts `Xvfb` in a sandbox of its own (`lnk sandbox run`), as
Link browser runs Chromium, rather than with your environment. It has no
network, reads its program and the display's cookie, and writes only X's
folder of sockets, `/tmp/.X11-unix`, where it makes the display's socket
for programs to reach. Its keymaps and lock are in a `/tmp` of its own,
so the tool holds the display's lock in your `/tmp` for it, and removes
the lock and socket when the display ends.

### The display is a door

Every `run` sandbox of the agent reaches the display's socket and
cookie, so the display joins those sandboxes to each other and to the
display's own.

- A program one call starts sees, and types into, the windows of a
  program another call started.
- Each speaks the X protocol to the server, so a flaw in `Xvfb` is a way
  from a `run` sandbox into the display's sandbox, which reaches what
  that sandbox does. Nothing else of `/tmp/.X11-unix` reaches a `run`
  sandbox.
- The display's sandbox writes X's whole folder of sockets, which every
  X server of this computer shares, so an `Xvfb` taken over could
  replace another of your displays' sockets there.
- The display takes no client without its cookie, so no other user's
  program reaches it. The cookie is new each time the display's server
  starts, and the tool sends it only to a server your user or root runs.
- The tool runs `Xvfb` only from the system's folders, owned and
  written only by root, never from the `PATH`.
