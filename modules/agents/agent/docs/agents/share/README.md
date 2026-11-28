# Create a Shareable Agent

> Make Any Agent and Share It

People share agents the way they share open-source code: start from one someone made, make it your own, and pass it on to your friends or publish it for everyone. One archive file carries an agent from one person to another, with [`lnk archive export`](../../../../../tooling/archive/docs/tooling/archive/README.md#export) and [`lnk archive import`](../../../../../tooling/archive/docs/tooling/archive/README.md#import).

```sh
lnk archive export --select agent.main
lnk archive import link-coach.tar.gz
lnk agent start --agent coach
```

## Start From a Community Agent

The quickest way to a good agent is one someone already made. Local
Link's [Ready-Made Agents](../../../../../../agents/docs/agents/ready-made/README.md), a programmer, a
researcher, a coach, and a personal assistant, are yours to import and
make your own. An agent a friend or your community made
comes in the same way, and [Import an Agent](import.md) says what import
checks and what the agent needs before it starts.

## Why Sharing Matters

Open-source software got good because people gave their work away and
built on each other's. Agents can grow the same way. If you can run
Local Link, you can make an agent and hand it to anyone else who runs
it, and they can improve it and pass it on.

## Make an Agent Worth Sharing

An agent is plain files you can read and change: its instructions,
skills, tools, sandbox, and Home. [Make an Agent Worth Sharing](worth-sharing.md)
shows how to get the most out of each, whether you started the agent from
scratch, imported it, or mean to give it away.

## Share Your Agent

One archive file carries an agent's state, and its credentials,
approvals, and installed programs stay on your machine.
[Share Your Agent](export.md) says what export carries, what it refuses,
and how to prepare an agent for someone else.

## Where People Share

An archive is one file, so you share it however you already share files,
in a message, by email, or on a shared drive. A community that keeps its
agents together keeps each one unpacked, as a folder in a Git repository,
where anyone can read every instruction and propose a change.
[`lnk archive export --from`](../../../../../tooling/archive/docs/tooling/archive/README.md#export)
packs such a folder into one archive, the way Local Link packs its
ready-made agents.

<!-- docs-tree:start -->

## Docs

- [Make an Agent Worth Sharing](worth-sharing.md): Get the most out of an agent by shaping its instructions, skills, tools, and Home, whether you started it from scratch, imported it, or mean to give it away
- [Share Your Agent](export.md): Export an agent you made to one archive file, and give it to your friends or your community
- [Import an Agent](import.md): Import an agent a friend, your community, or Local Link made, and start it as your own

<!-- docs-tree:end -->
