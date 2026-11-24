# Decisions

Why environments work the way they do.

## The file

These choices keep setup portable while preserving the distinction
between the machine, its users and the work that an environment runs.

### Why is this public, and in its own module?

The file and `lnk env` on your own boxes are useful to anyone with a
cloud account, so they're public, and they name no cloud: a box is `lnk
box`'s, on whichever account, reached by running `lnk`. Its crates know
no box, cloud or job: what a job adds (its record, its box made for it,
a cloud that runs it) is the job module's, which hands an environment
the stages to run.

### Why "environment" and "job"?

An environment is what a machine needs before work starts. A job is an
environment and a command, run to the end, that can be cancelled and
says how it ended; job, task and run mean the same, and job is the
word. CI is a job started by a push, a scheduled report one started by
the clock.

### Why is a path given to every command?

The file lives in the repository it sets up, and a repository can hold
several. Every command takes its path (`--env`, or the file itself), so
nothing is guessed from the folder you're in.

### Why do the image's stages run from the home folder, and the rest from `workdir`?

A stage saved in the image sets the machine up, and the first often
makes the folder the work happens in: a clone run from a `workdir` that
isn't there yet would have nowhere to start. So those start at the home
folder and name their paths from it. Everything run at every start, a
stage with `when = "start"`, a service, its `ready` and `reset`, a job's
stages and an agent, comes after the image and works on what it made:
it starts at `workdir`, which is then there, or fails naming it. A
relative path in a service and in the job it serves names the same
file.

### Why do services run in Docker only on a box of their own?

The sandbox refuses Docker's socket, which runs whatever it's sent
outside the sandbox. A service that needs Docker runs as root, which
gives its job a box of its own.

### Why do some names stay as they were?

The agent specification's `[instructions] environment` describes its files and
connections, which an environment sets up. Each name describes its
subject, and its page defines that meaning.

### Why does root say a machine is a box?

A box runs an environment's root stages, and your computer never does,
so what says which must be something no program of yours can change:
an agent with no sandbox, a script you ran, or another user of a shared
machine. `/etc/lnk/kind.toml` is written as root at a box's first
boot, before anything of yours runs there, and refused when anyone but
root could write it; whoever can write it as the machine's root could
run anything as root anyway.

#### User namespaces

In a user namespace of one's own, a file mounted over it seems root's: `lnk` checks its user IDs are the
machine's, a best effort the same namespace can fake, and what holds
is that a root run goes through `sudo`, which gains no root there.

#### A missing marker

Without it a machine is a computer, so a mistake keeps root stages
off, never on. `~/.lnk/safe/installation.toml` is yours, named apart from
root's so the line between them shows, and holds the machine's ID and
name and your rules for it, none of which loosens anything root does.

#### Images and credentials

The file says the box's OS and processor
too, since an image built on it is for those. An image built on a box
(`lnk env image create here`) doesn't save itself: saving is its cloud
account's, which is the computer's that made the box, since a box holds
no cloud credential. So the stages run on the box, and it says the `lnk
box image create` that computer runs to keep it.

## Why does every command take the file as `--env`, and the place as `--on`?

One grammar for every command: the environment file is always `--env`,
and the place a command acts on, a cloud account, a box or `here`, is
always `--on`, as [`lnk agent create --on`](../../../../../agents/agent/docs/agents/start/create.md#create-on-a-hosting-account) takes it, never a word whose
meaning hangs on what it names. A command's word is its object: the
service `lnk env reset redis --on web-1` resets, which the box knows
the environment of, since it was made with it. Making a box with an
environment is [`lnk box create --env`](../../../../box/docs/services/boxes/guide.md#create-a-box)'s, as every machine is made by
`lnk box create`; [`lnk box start`](../../../../box/docs/services/boxes/lifecycle/README.md#start-a-box) starts a stopped one. Current commands use this grammar.

### Why parallel groups?

A stage may group independent commands under `parallel`. Members start
together, each in its own sandbox; all finish before the next stage.
The first failure received determines the group's result. A group has
one deadline within the overall build or job limit, alongside each
member's own timeout. Group shape and order are part of the image hash.

## The environment specification

The specification separates setup from allocation and workload. Each
key has an owner, and the file carries portable setup rather than
another module's policy.

### Why machines, users, environments and runs?

Each subject has one owner: a machine is a computer or box; users belong
to that machine; an environment defines what a machine made from it has;
a run is a command executed as a user in a sandbox. Stages, services, job
stages, agents and SSH sessions are runs with different start and end
conditions. A job and an agent therefore work in the same environment.

### Why aren't the box, the job and the repository in it?

An environment describes its setup. Machine size belongs to a box specification,
and a job's stages belong to its job specification. Cloning a repository is a setup
stage, using the same network rules as any other command.
Multiple repositories are multiple stages. Unsupported keys are refused
instead of silently changing the meaning of a file.

### Why is the OS optional on an environment?

A box has an OS always (its specification's `os`, which has a default). An
environment that names none runs on any, so an agent in it moves
between machines of different ones; its image is built for each OS and
processor it's asked on, the hash saying which. One that names an OS
refuses a box of another, and `here` on another, rather than build
something that isn't what it says.

### Why `user`, and not `root = true`?

A box has many users: its own, `root`, the people let in with [`lnk box`](../../../../box/docs/services/boxes/README.md#boxes)'s
`user add`, and those a stage makes. `root` is one of them, so a run
names its user, and `root` runs outside any sandbox because it must.
Who comes to a box is the box's to say, never the environment's, which
says what a machine has.

### Why does only a box take an image?

An image is a machine saved whole, and only a box is made to be saved:
your computer holds you, your keys and everything else that's yours.
So `lnk env image create` runs on a box, and `lnk env`'s `setup` runs the build
stages on your computer without saving anything, refusing `root`'s,
which would change the machine outside any sandbox.

### Why is an agent's sandbox its own file?

An agent's sandbox follows it between machines and is granted through
[`lnk agent permission add`](../../../../../agents/agent/docs/security/sandbox/permissions/README.md#choose-what-your-agent-can-reach). Its own file defines what the agent may do, never a
stage's.

### Why does the environment take no flags?

Its stages and services are lists of tables, which a command line
doesn't hold, and a person writes an environment once and keeps it
beside the code it sets up. So `--env <file>` names it everywhere, and
`lnk env`'s `spec` prints it back.

### Why are its paths printed relative?

The sandbox specifications and inputs an environment names are copied beside
it to each box, and found there from its folder. Absolute paths would
name your computer's folders on a box.
