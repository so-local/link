# Decisions

Why skills are copied, checked and supplied to the harness this way.

## How does a skill reach an agent?

`lnk agent skill add` copies its folder into the agent's `skills/`, so a
skill is a file of the agent's like any other: it moves with the agent,
a backup keeps it, and you can edit or delete it there. It's named as
its `SKILL.md` says, as the Agent Skills standard does, and checked
before anything is copied: a folder that isn't a skill is refused,
saying why.

### From a URL

A URL is a git repository, cloned once with your own git into a private
folder of the system's, never the agent's, since skills are shared as
repositories, GitHub's page of a folder naming one in it.

### How it's copied

A copy leaves out links and anything that isn't a file or folder, takes
at most 1,000 files, 64 MB and 1,000 folders at most 32 deep, and is put
in place by a rename, so a harness never lists a skill half copied.

### Never through a link

The harness writes `skills/` while `skill add` runs outside its sandbox,
so every file and folder is read and written through its folder's handle
without following a link: one swapped in, in the skill or where it's
copied to, is refused, and a repository's folder that's a committed link
can't lead out of the clone. A harness that doesn't take Link's
instructions doesn't read `skills/`, and `skill add` says so.

### In the prompt

Skills are listed in the prompt whether or not memory is on, so the
prompt stays the same across conversations, a narrowed subagent's
included. Memory is opt-in, so `skill add` says when it's off: the one
place it's seen.
