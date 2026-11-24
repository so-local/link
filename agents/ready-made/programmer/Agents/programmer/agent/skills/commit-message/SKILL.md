---
name: commit-message
description: Commit one logical change with a message that says what and why, in the repository's own style.
---

# Commit

1. **Check what you commit.** `git status` and `git diff`: only this
   change, no build output, secrets or editor files. Stage by name
   (`git add path`), not `git add -A` blindly. Be on your own branch,
   never the default one.
2. **Follow the style.** Read `git log --oneline -10` and the repository's
   guide: Conventional Commits (`fix(parser): ...`), an issue prefix, a
   sign-off (`git commit -s`) or plain sentences. Match it.
3. **Subject:** at most 72 characters, imperative mood ("Fix", "Add", not
   "Fixed"), saying what the change does, no trailing period.
4. **Body** after a blank line, wrapped near 72 columns: why the change
   was needed, what approach you took and why, side effects, and how it
   was tested. Reference the issue if one was named. Explain the reasoning,
   not the diff.
5. **One logical change per commit.** If the diff holds two, commit them
   separately (`git add -p` is interactive and not available; stage whole
   files, or split the work).
6. Commit with a heredoc or `-m` per paragraph, then `git log -1 --stat`
   to confirm. Never use `--no-verify` to bypass hooks; fix what they
   report. Never amend or rewrite a commit already pushed.
7. Add any attribution lines your instructions or the repository ask for;
   otherwise do not mention the tools or session that wrote the change.

Commit with `git commit` alone: never add or change a hook or git's
settings to make it pass. If the commit fails because `.git` is read
only (the person took back that grant), do not look for a way around it:
leave the changes in the working tree, and put the branch name, the files
and the finished message in your report so the person can commit them.

Do not push unless asked.
