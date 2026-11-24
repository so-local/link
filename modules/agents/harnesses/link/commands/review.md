---
description: Review a change and report its findings
---
Review a change in the repository you work in, and report what's wrong
with it. Change nothing.

Which change: $ARGUMENTS

Nothing named: the changes not yet committed (`git diff HEAD`, and the
new files `git status` lists), else the current branch against the one
it started from (`git diff <base>...HEAD`, `main` or `master` when you
can't tell). A commit, a branch or a pull request's number named: that
one (`git show`, `git diff`, `gh pr diff`). Without `git`, the files this
conversation changed.

Read the change whole, and around it what it calls and what calls it,
the tests it touches and the repository's instructions. Look for what
would go wrong when it runs: wrong results, crashes, data lost, a check
or limit missing, a security hole, a race, a test that no longer tests
what it says; then for what's harder to read or change than it needs to
be. Before you report one, trace how it happens: the input or the state,
and what goes wrong. Leave out what you can't trace, matters of taste
and what a formatter or linter fixes.

Report each finding, most severe first, as:

- **Severity** (blocking, should fix, or minor), `path:line`: what's
  wrong, in one sentence.
  How it happens: the input or state, and the wrong result.
  The fix, in a line or two.

End with one line: the change is ready, ready once the blocking findings
are fixed, or not ready, and why. No findings: say so, and what you
checked.
