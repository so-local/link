# Decisions

Why [Link Browser](../README.md) drives a browser of its own, in a sandbox,
and leaves the network rule to the sandbox's proxy.

## Why does it use the sandbox's proxy rather than a proxy of its own?

Link Web already refuses a page on this computer or its networks unless a
host is named, and the sandbox's proxy keeps the same rule for everything
it carries. Starting the browser in a sandbox open to the agent's own
internet hosts therefore gives one rule in one place, so a link, a redirect, or a script is held to
it as a fetch is. A second proxy inside the tool would have had to repeat
the rule and stay in step with it. A named private host is the sandbox's
`--lan.hosts`, and a port of this computer its `--localhost.ports`.

## Why does the browser reach only the agent's hosts?

A tool that reached every host would widen an agent limited to a few,
or to none, to the whole internet the moment the browser was added. The
browser inherits the agent's `[internet] hosts` instead, and reaching
any site takes an explicit `*` grant, which the agent's permissions then
show.

## Why does each conversation have a page of its own?

One shared page let a page, what was typed into it, and `close` cross
from one conversation to the next, and from one person to another. Each
conversation now has its own tab. Only a turn answering the owner opens
in the profile's context, the one holding the agent's sign-ins, so a
member in a shared chat or a stranger never acts as the owner on a site.
The cost is that a sign-in made in another's turn doesn't last.

## Why does a shared agent use no sign-ins?

An agent shared in a chat with members or on a team uses the sign-ins
in no conversation, the owner's included. Its browser starts with the
lowest standing any of its conversations may have, a member's, and
holds every turn to it. Proving that a turn is the owner's, past that
floor, would need the sandbox's bridge to vouch for it. Failing closed
keeps a member from ever acting as the owner on a site, at the cost of
sign-ins for a shared agent.

## Why are a page's questions dismissed?

A `confirm()` or a question before leaving the page is often the last
step before something that can't be undone, such as deleting an account.
Dismissing it by default means a click never confirms unseen, and the
agent answers OK only by asking for it with `dialog`, after reading the
question. An alert offers only OK, so it gets OK.

## Chromium

Which Chromium the browser starts, and how old it may be.

### Why does it refuse an old Chromium?

On Linux Chromium runs without its own sandbox inside Link's, so a hole
in its renderer reaches all of the browser's sandbox, the profile
included. Refusing a version older than `minimum_major` in
`src/browser.toml` keeps the known holes of old releases out. A pinned
build Link includes would close this further, and is later work.

### Why does the agent's specification name the browser's program?

The search finds a browser by its usual names and resolves the
distribution launchers it knows to their engines. A Chromium installed
elsewhere, or started through a launcher script of its own, has no way
in by the search: the script would run inside the sandbox, which refuses
each program it calls. `[tool.browser] program` in the agent's specification
names the program instead of the search. It's control, so it's read
from the specification alone, never from the agent's files or the environment,
and the browser allows it in its sandbox as it allows a program it finds.

### Why is a named program checked at each start?

A named program must be an absolute path to an executable file that
isn't a script. A script is refused by name, since resolving an unknown
launcher would mean reading and running its host configuration outside
the sandbox. The specification's own check holds only what holds on every
machine, the path being absolute, so a specification moved to another machine
still reads, and the file is checked where the browser starts.

## Why do DevTools speak over a pipe?

[`lnk sandbox run`](../../../../../../../../security/sandbox/docs/security/sandbox/guide.md#sandbox-guide) closes every descriptor but the ones it is given, so the
browser is started with `--remote-debugging-pipe` and its DevTools on
descriptors 3 and 4, mapped to the sandbox's standard input and output.
That leaves no port and no WebSocket for another program to reach.

## Why does the browser run in a sandbox when Chromium has one?

A browser's own sandbox is not Link's to promise, and on Linux it can't
start inside Link's: Chromium's sandbox needs a user namespace, which
Link's sandbox refuses to any program it runs, so it runs there with
`--no-sandbox`. Link's sandbox is the wall: the browser reads its
program and writes only its profile.

## Why does each agent have a profile of its own?

A sign-in should last from one session to the next, and a browser that
shared the owner's profile would hand the agent every sign-in the owner
has. The profile is in the agent's folder, which no harness reaches, and
the tool refuses the supported browsers’ known personal profile
locations. A manually configured custom profile must also be separate.

## Why does a page the sandbox refused still pause the browser?

A page touching a forbidden address, such as a cloud's metadata
service, counts toward the sandbox's pause like any program's refusal,
so a page can't probe without consequence. The browser says the wire is
paused and how to resume it, rather than blaming the host.
