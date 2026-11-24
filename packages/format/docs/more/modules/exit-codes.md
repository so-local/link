# Exit Codes

What each code `lnk` and its modules exit with means, so a script can
tell a refusal from a failure. Each code has one meaning, whichever
module gives it.

## The Codes

Every code Link gives, smallest first: the shared ones, then each
module's own, then the sandbox's and an interrupt's, as a shell's.

| Code | Meaning |
|---|---|
| 0 | it did what it's for |
| 1 | it failed |
| 2 | the command line was wrong, or a question had no answer |
| 3 | a sandbox, specification, policy, or service refused it |
| 4 | the module it needs isn't installed |
| 10 | a connection's harness isn't linked |
| 11 | a connection's service refused its token |
| 12 | a connection's service refused a reply |
| 13 | a webhook was refused |
| 126 | the sandbox couldn't run the program |
| 127 | the sandbox couldn't find the program |
| 130 | it was stopped with Ctrl-C |

## A Program's Own Code

A command that runs another program, such as [`lnk sandbox run`](../../../../../modules/security/sandbox/docs/security/sandbox/guide.md#sandbox-guide), ends
with that program's code, as a shell would. That code is the program's
and passes through untouched, so it can be any number, these included.

## Where the List Is Kept

The list is `link-format`'s `Exit`, and a module's own code is declared
there once with its meaning, so two modules can't give one number two
meanings. A test holds this page's table to that list.
