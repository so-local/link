# Share Your Agent

Export an agent you made to one archive file, and give it to your friends or your community. Whoever imports the archive gets a copy of their own, with an ID of its own.

```sh
lnk agent stop --agent main
lnk archive export --select agent.main
```

## Prepare an Agent for Someone Else

An archive holds everything the agent has, its chats and Home included,
and export lists what it holds before you send it. Share an agent you
made for sharing rather than the one you talk to every day, and read its
Home and instructions for anything private first.
[Make an Agent Worth Sharing](worth-sharing.md#test-as-a-stranger-would)
shows how to try it as its recipient will.

## What an Archive Carries

An agent's archive carries its whole state, its two folders under
`~/Link`: its settings and sandbox file from
`~/Link/Safe/Agents/<name>/`, and its instructions, skills, chats,
memory and Home from `~/Link/Agents/<name>/`. Its credentials,
connections' tokens, installed harness and approvals stay behind, since
none of them is under `~/Link`, and so do its ID and ports, which are
this machine's records.

### Chats Without This Machine's Work

A chat records which work each model request and proposed call was part
of, naming the agent's ID, its session and its attempt on this machine.
Nothing reading a chat needs that work, so export carries each such chat
as a log of its events with that work left out, and an import reads
every event as it was written. A chat kept compact goes as that log
too. The chats on this machine stay as they are, as
[`archive.carry`](../../../../../tooling/archive/contract/docs/tooling/archive/more/questions.md#archivecarry)
describes.

## Stopped, Here, on Link Harness

Export reads files and nothing else, so it carries an agent only when its
files are all of it. A running agent is refused, naming `lnk agent stop`,
and so is an agent on a box or a cloud, and an agent on a harness other
than Link Harness, whose own state is in that harness's home. Its refusal
names [`lnk agent harness set link`](../harnesses/use/README.md), which
brings its chats and memory to Link Harness so it can be exported. A file
in its settings folder other than `agent.toml` and `sandbox.toml` is
refused too.

<!-- docs-tree:start -->
<!-- docs-tree:end -->
