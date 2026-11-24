---
name: dependencies
description: Add, upgrade or remove a dependency safely, with the right tool, the lockfile and a check of what you pulled in.
---

# Dependencies

## Before adding

Ask: does the standard library or an existing dependency already do this?
A new dependency is code you ship and maintain. Ask first if the
repository's guide is silent and the change is not trivial. Prefer
widely used, maintained, permissively licensed packages. Check the
name carefully for typosquats, and the license against the repository's.

## Add, upgrade, remove with the project's own tool

Never edit lockfiles by hand; let the tool write them and commit manifest
and lockfile together.

Read the repository's manifests, lockfiles, and pinned manager version.
Use its documented add, upgrade, or remove command and reproducible install
mode, with toolchain details from the matching language skill. Keep packages
in the project's environment, never install system libraries, and never mix
package managers.

## Upgrading

Read the changelog for breaking changes. Upgrade one dependency (or one
family) at a time, build, run the tests, and fix the call sites. Separate
commit per upgrade. Prefer the smallest version bump that solves the
problem.

## Check

Run the repository's dependency audit tool if installed. Look at what the lockfile
diff pulled in transitively. Remove dependencies you no longer use, and
report what you added and why.
