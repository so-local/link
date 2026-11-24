# Decisions

Why `lnk-deepseek` runs DeepSeek Harness the way it does. Link's own decisions about
harnesses are on its [Agents Decisions](../../../../../../../../modules/agents/agent/docs/agents/more/decisions.md) page.

## How does Link talk to DeepSeek Harness?

Over the [Agent Client Protocol (ACP)](https://agentclientprotocol.com),
as it talks to every harness. `dsh` speaks no chat connection, so
[adapter endpoint](more/runtime.md) is its endpoint, the [harness hub](../../../../../../../../modules/agents/harnesses/portability/hub/docs/agents/harnesses/more/protocol/hub.md)
relaying Link's ACP to one `dsh --profile acp` over stdio, because that
profile keeps many sessions in one process and takes one up again after
a restart, where its headless profile starts a process per message and
its SDK profile can't resume. Each chat is a session, named in
`threads.json`.

## Why are DeepSeek Harness's own sandbox and approvals off?

Because Link's sandbox confines it, and nobody is there to approve. Its
own sandbox is bubblewrap or Landlock on Linux and Seatbelt on a Mac,
which can't nest inside Link's; where it can't start, `dsh` refuses to
run a command at all. Its approvals would wait on a person who reads
the chat, not a prompt. Link's sandbox gives it the same bounds: its
home and the files folder, and the network through Link's proxy.

## Why does Link turn off what DeepSeek Harness sends DeepSeek?

Because you didn't ask for it. By default `dsh` attaches the session's
log and its list of modules to each request on DeepSeek's route, even
to another address, and keeps its telemetry and web search on DeepSeek's
servers. Link reaches the model through your own provider, so none of
those rows is needed. Its web search is off with them, since it needs a
DeepSeek key.

## How does Link run and check DeepSeek Harness?

[adapter endpoint](more/runtime.md) answers ACP at port 18820's `/acp` and runs one
`dsh --profile acp` behind it, started again if it stops. A thread's
turns run one at a time. `dsh` has 2 minutes to answer ACP's
`initialize`, and starting or taking up a thread's session 2 minutes
too, then the message fails and the next one tries again.

### `/health` and `/_link/proof`

`GET 127.0.0.1:18820/health` answers once `dsh` does, waiting up to 3
seconds for one that is starting. `GET 127.0.0.1:18820/_link/proof` is
where it proves to Link's connections that it holds the endpoint's key
before they send it.

## Where are the proxy's addresses for `dsh`?

In Link's patch over its settings, `~/.deepseek/link.patch.yml`, mode
600: a local model's address holds the proxy's secret. Tools stay in
`link-tools.json`, also mode 600, and are handed to each ACP session.
`dsh` is given only the patch's path (`--patch`), so no secret is on a
command line.

## What portable state does DeepSeek Harness carry?

Conversations, instructions and skills, so a switch keeps what the agent
was told and what it learned to do.

### Conversations

The adapter reads DeepSeek's native session logs and retains their original
records alongside readable projections. Its native runtime cannot
reconstruct foreign sessions, and its ACP profile has no `session/load`, so
the hub supplies archive-backed continuation and reports what remains
archival. Writing an imported conversation into its native logs isn't
done: one log in the wrong compression makes `dsh` refuse its whole
sessions folder.

### Instructions and Skills

`dsh` reads a user-level `AGENTS.md` in its home at each session's start,
and scans Agent Skills folders. At each start the adapter writes the
agent's persona, owner and standing instructions into Link's block of that
`AGENTS.md`, and adds the skills Link copies to the folders `dsh` scans.
Out of it, the file's text outside Link's block and the skills in its own
`skills` folder are exported; coming in, they're written to the same
places.

### Memory, Tools and Schedules

`dsh` keeps no memory of its own, so Link Memory reaches it as a tool. It
takes MCP servers with each ACP session, from the agent's specification.
Its ACP profile runs no scheduler, so Link Scheduler owns the agent's
schedules.
