---
name: write-tests
description: Write fast, focused tests test-first, in the repository's own framework, and never weaken one to pass.
---

# Write Tests

## Process

1. Find how the repository tests this area: location, naming, framework,
   fixtures, helpers. Copy that pattern; do not introduce a new framework.
2. Write the test first and watch it fail for the right reason (the
   behavior is missing, not a typo). A test that never failed proves nothing.
3. Make the smallest change that passes it, then run the area's tests.
4. Add the edges: empty, single, many, boundary, invalid input, error
   paths, and the bug's exact trigger for a fix.

## A good test

- Tests one behavior, named for it (`rejects_empty_name`, not `test1`).
- Asserts outcomes the caller sees, not internal calls or private state.
- Is deterministic: no real clock, network, random seed, ordering or
  shared global state. Inject time and randomness; use temp directories
  that clean themselves up; use ephemeral ports.
- Is fast (milliseconds). Mock only the slow or external edge, never the
  code under test.
- Fails with a message that says what was expected and what happened.

## Never

Skip, ignore, delete or loosen a test, widen a tolerance, add sleeps or
retries, or special-case the test in production code to get green. If a
test is wrong, say why and fix it deliberately. If a test is flaky, report
it rather than hiding it.

## Repository Commands

Use the repository's own runner, fixtures, isolation helpers, and naming
conventions. The matching language skill supplies toolchain details. Read
the runner's supported selection flags before choosing the narrowest test.

Check coverage of your change by reading it against the diff; run the
mutation in your head: would the test fail if the change were reverted?
