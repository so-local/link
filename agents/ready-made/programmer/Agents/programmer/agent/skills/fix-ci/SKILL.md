---
name: fix-ci
description: Fix a failing CI run by reading its log, reproducing the failing step locally and fixing the cause.
---

# Fix CI

1. **Read the log.** Find the first failing step and its first error, not
   the last line. If you are given a log or a URL's text, use it; you
   cannot reach a CI service unless a tool is provided, so ask for the log
   if you lack it.
2. **Find the exact command.** Open `.github/workflows/*.yml`,
   `.gitlab-ci.yml`, `Jenkinsfile`, `.circleci/config.yml` or the
   Makefile target it calls. Note the toolchain versions, environment
   variables, working directory and flags.
3. **Reproduce locally** with the same command and versions as far as the
   machine allows. Use the matching language skill to verify the toolchain
   versions. If a version differs, say so.
4. **Classify** the failure:
   - a real bug or broken test: fix the code (debug skill);
   - formatting or lint: run the formatter or linter the CI runs;
   - dependency or lockfile drift: regenerate the lockfile with the
     project's tool, never by hand (dependencies skill);
   - environment (missing tool, network, version): fix the config or
     report it;
   - flaky (passes on a rerun): report it and do not hide it with retries.
5. **Fix the cause.** Never disable a job, skip or delete a test, lower a
   threshold, or add `|| true` to make CI pass.
6. **Verify** by running the failing command locally to green, then the
   repository's full checks. Say which CI steps you could not run locally.
7. Commit with the commit-message skill. Do not push unless asked.
