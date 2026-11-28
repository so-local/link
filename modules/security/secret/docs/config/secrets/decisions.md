# Decisions

Why `lnk secret` works the way it does.

## Keeper storage and routes

Why the keeper's store and the routes that open it are made as they
are, so no copy of a disk or of a route's file gives a secret away.

### Why seal the keeper's store to a TPM?

A wrapping secret on the same disk would travel with a stolen copy.
The TPM instead seals a small recipient to its own hardware; disk holds
only its opaque blobs and encrypted store. Existing stores migrate when
opened, under a lock. A TPM failure refuses the operation, while a machine
without a TPM retains its reported private-file fallback.

### Why bind an encrypted route to a UID and a fresh grant identity?

A proxy must carry ciphertext without holding the credential. The keeper
opens it only for the socket's actual user and the current bridge proof.
A fresh identity on each grant also makes retained ciphertext unusable
after revocation, even if the same proof is registered again. Root may
bind for a helper's user; a working user may bind only for itself.

### Why does a route name a secret, never its value?

So a secret's value is in its store and nowhere else on the disk. The
bridge that owns the store reads the value at each call, after checking
the grant names the route's caller, so a removed secret or grant stops
the next call. Already dispatched requests can finish. A run handed
secrets by its caller, with [`lnk sandbox run --with-fd`](../../../../sandbox/docs/tooling/sandbox/guide.md#sandbox-guide), is granted nothing
by the store, so its values go to its proxy on a pipe, never in a file.

## Why is a secret given only where it's granted?

Whoever can edit a file can name a secret in it: a dependency, an agent
writing in the workdir, a pull request's branch. If naming a secret
were enough to be given it, any file run on this computer could take
any secret kept here. So the grant is kept with the value, on this
computer, and a file only names what it's already been granted.



## Why `add`?

A secret's value is made somewhere else (by npm, Stripe or GitHub), and
Link takes it on with `lnk secret create`. Creating it again replaces
the value and grants under a new ID. `lnk secret add` grants an existing
secret to an agent without replacing its value.

## Why does how a secret is given live with its value?

A sealed secret's host is where its value may go. If whatever names
it set its host, whoever edits that could send it elsewhere. So the
host, the header and the format are set with the value, by `lnk secret
create`, and nothing else changes them.

## Why is an agent given a secret sealed only?

An agent's harness does what its model decides: a value in its
environment is the model's to read and to send wherever its sandbox
reaches. Sealed, the agent's proxy adds the value to calls for its one
host, outside the sandbox, and the agent holds a placeholder, so it can
use the secret and never take it.

### And a plain value?

Every secret an agent needs so far is an HTTPS call to one host, which
sealing covers. Should one need a plain
value, it would be handed to the harness by a stage of Link's own
between its background service and its sandbox, never in the service's
definition, which other users of the computer can read.

## IDs

Why a secret, who it's granted to and an agent's own are each known by
an ID, never a name.

### Why is a secret kept by its ID?

A secret's name is free again once it's deleted. A grant or a move's
copy noted by the name would reach the next secret made with it, so
each secret is kept under an ID minted when it's created and retired
when it's deleted, which no other secret here is ever given. Its name
is a label for commands and for what runs, and its Keychain account is
its ID's (`secret/<id>`), so a secret made again never finds the first
one's value. A store an earlier `lnk` wrote, by names alone, is refused
rather than converted: nothing was published before IDs, so no reader of
the old shape is kept.

### Why does a grant name a service, by ID?

An agent's name is free again once it's removed. A grant to the name
would hand a removed agent's secrets to the next agent given it, so the
grant keeps an ID that's never given again. It names who acts, the
service the agent acts as, `service:<id>`, never the agent's own ID,
which names what runs. A grant is a principal, an action and a
resource, the shape any check of who may have what reads, so a later
release takes the saved grants as they are.

### Why is an environment given nothing here?

An environment you run acts as you, so there's no one else to grant:
it's given what its file asks for of the secrets you hold, once you've
said yes, and the environment module keeps that yes. The secret module
names no environment or job, and reads you a secret only as its holder.

### Why does an agent's own secret have no name?

A name built from the agent's (`AGENT_MAIN_MODEL_KEY`) would change with
a rename, and meet another agent's when two names run together. So an
agent's own secret is found by the agent's service and its use, and
never granted by hand to anything else.

### Why is a grant shown with its agent's ID?

A grant shown as `agent:main` would read as an ID while it holds a name,
and a name is free again once its agent is deleted. So a grant keeps
the agent's own ID beside its name, to show alone, and is shown as
`main (agent:<id>)`. The grant still names who acts, the agent's
service: the agent's ID names what runs and grants nothing.

## Why does a Keychain failure stop the command?

A value the Keychain can't read, or won't keep, could be read as a
secret that isn't there, or kept in a file instead. Either hides a
locked Keychain behind a quiet change. So a Keychain error stops the
command, and each computer has one backend, chosen once: the Keychain
on a Mac and a file in the Safe elsewhere. The one exception is the old
item a secret made again leaves behind: the new secret is kept, so a
Keychain that won't delete the old item gets a note naming it, never a
failed create.

## Why are an agent's keys refused where Link kept them before?

Moving them would mean code that reads every old place, kept for as long
as anyone might still have one. So a key still there is named when the
agent starts, with how to give it again, and nothing removes it: you
remove it there when you choose.

## Copies a move gives

Why the copies a move sends to a box or a cloud account are noted, and
how `delete` reaches them.

### Why is a copy noted before it's sent?

A move sends an agent's keys, sealed, to the machine it goes to. A
note written after the switch would miss a switch cut short, where the
other end may have received the keys and kept them. So the move notes
the place first, on the computer running it, and the note says where a
copy may be. A note for a place that has no copy costs a `remove` that
finds nothing there.

### Why does `remove` exit non-zero for a box it couldn't reach?

The point of `lnk secret delete` is to remove the stored copies it can reach. A
box that's stopped still has its disk, and a key on that disk is still
usable. So the box stays noted and `delete` fails until a later run
reaches it, rather than saying it's done when it isn't.

### Why is a copy noted by IDs and by machine?

A name is free again once its agent is removed. Noted by name alone, the
copies of a removed agent and of a new one of its name would be one
note, and one's moves would change the other's. A box's name is free
again too, once the box is removed. So each copy is noted under its
secret's ID with the agent's ID and the secret's use, and a box by its
installation ID as well as its name. `delete` asks a box to remove only that
agent's secret for that use, which the box keeps under an ID of its own,
and never touches a box made again under the name.

### Why is a cloud account's copy left until its agent goes?

A cloud account that hosts agents keeps an agent's keys with the agent,
and Link's contract with it has no way to forget one key. So `delete` says where the key is and that it goes with the
agent there. This computer keeps the note until the agent is removed
there or moves away.
