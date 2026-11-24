# Writing Hooks

A hook is the one line under a section's plain title. The marketing
website can show it as its heading; documentation shows the short title. It has to make a reader want the page. Read this
before writing or changing one.

## What a hook does

A hook sells the part to someone deciding whether to read on. It says
what the reader gets, not how the part works, and has a turn to it: a
contrast, a twist on a familiar phrase, or a limit said as a boast.
It's clever whether the part is Link's own or another product's, since
a hook for a third party's harness or chat app sells their software
too.

## What the reader sees, never how it works

A hook starts from what the product already is in its users' lives,
and sells what the agent becomes there. Slack is where a team works, a
tool and a connection, not "your chat"; a phone's messages are what
you'd text a friend. So a hook never leans on a mechanism or a setup
step, however distinctive: BotFather, Socket Mode, a port nothing
listens on. Those belong in the page.

## A turn, not a fact

The turn is real wordplay or a contrast a reader smiles at: a twist on
a phrase they know, an old word made new, two halves that pull against
each other. A feature said plainly, even a good one ("with nothing on
your machine listening", "now with your agent"), is a fact, not a
hook. If a hook would still read true of a dozen other products, or
reads like the page's first sentence cut short, write it again.

> Approved: "Slack's newest coworker never logs off" (where the team
> works), "Telegrams your agent answers in seconds" (the old word made
> new), "Text it like a friend, it works like staff" (a contrast),
> "Link Harness is small and mighty" (two words that pull apart),
> "Webhooks let your apps nudge your agent" (true to what it is),
> "DeepSeek, a whale that fits in a sandbox" (its logo meets its home),
> "Hermes, the messenger, speaks any model" (the name made true),
> "OpenClaw has a personality, and a firm grip" (a claw, never named).

## For every reader

Write for a reader of 13 and up. A turn borrows from school, work,
sport, travel or home, never from smoking, drinking, gambling, hunting,
fishing, firearms, violence, blood, horror, cursing,
religion or politics.

## Clever, but true

The turn rides on what the thing is, never beside it. A reader who
knows only the hook should still guess what the page's part does: a
webhook is another app telling your agent something happened, so a
hook about who gets let in reads as a login page and misses it.

### Never Repeat the Name

The pun points at the name; it never says the name's word again. A
hook that repeats it ("DeepSeek goes deep", "a hand, well, a claw")
has no turn left: the reader makes the link, or there's no joke.

## Where a pun belongs

A pun works where the reader already knows the name: a product they've
seen the logo of (Slack, Discord, Telegram, Hermes, OpenClaw,
DeepSeek), whose page they open to connect their agent to it. Link's
own products may get one when it carries two of its values in a phrase
people know ("Link Harness is small and mighty"). Supporting parts,
such as stores, settings and contracts, get a dry, clear line.

## Never forced

When no turn fits, a clean, plain line is better than a forced one: say
what the reader gets, in their words, and stop. A pun that needs
explaining, or a joke that doesn't make sense, is worse than none.

### Read True From a Search

A hook is also what a search shows under the page's title. Someone who
lands on it from a search knows nothing of Link, so it may be cute, but
it uses the words they searched for and reads true on its own.

### Write Three, Keep One

Don't go hunting for puns. Write three, then keep the one that makes
the most sense to someone who has never heard of Link and just landed
on the page: a chuckle is a bonus, and "what did I just click on?" is
a miss.

## The Words

A hook's words follow a few fixed rules, so every hook on the site reads
the same way and none points at another's page.

### Names, spelled out

A reader may not know an acronym. Spell it out where it's met first, a
hook or a title ("Agent Client Protocol (ACP)", "Agent State Protocol (ASP)"), and
say "ACP" or "ASP" only after. A pun on a name rides the meaning a
reader already has: a messenger delivers.

### Keyword first

A reader scans the hooks down the page the way they scan a table's
first column. So a hook opens with its keyword, the word that names
what it's about: "Workshop behind every agent", not "The workshop
behind every agent" or "It builds the workshop". The checks fail a
hook that opens with "it", "its", "this", "a", "an" or "the", one
holding a colon (a label and its gloss, never one line), and one over
45 characters, since it's the page's heading and half its
tab's title.

### Title Case, and more than the name

The website makes a hook the page's title for search and sharing, so it
is in Title Case like every title: each word capitalized but the short
words inside it, "Workshop Behind Every Agent". A hook says more than
its page's name, at least three words: "Configuration" is a title, never a
hook. The checks fail both, `hook-case` and `hook-words`.

### No collisions

Each word a hook leans on means one thing across the whole site, so
two hooks never claim the same idea. Memory is Link Memory's, a search
over an agent's own files, so the stores' hook says where things are
kept without calling it memory. Before writing one, read the hooks of
the pages beside it and of every page whose feature is close, and keep
to the site's own names for things.

## Link's principles

A hook for one of Link's own parts may carry a principle, when one fits
naturally. These are what Link stands for, to draw on, not to recite:
your intelligence is your own; your agent answers only you; it moves to
another harness, model or machine in one command, with its memory,
files and chats; every program reaches only what you allow; the core
is small and its code readable; it runs in your own clouds, with your
own keys; it's open source, and its dependencies are ones we can trust.

## A few at a time

Hooks are written by one writer, so their words stay apart, and a few
at a time. Write two or three, show them to whoever owns the site, take
their feedback, and fold what it teaches into this guide before writing
the next few. Each section left to write is listed in
`tests/docs/large-files.toml`'s `unhooked`, and leaves the
list once its hook is in.
