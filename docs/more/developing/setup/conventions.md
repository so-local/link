# Conventions

The rules every change follows, from the first commit to the release.
How to write a doc is in [Writing Docs](../../../../tests/docs/docs/more/developing/documentation/writing-docs.md).
What each recurring file is, who reads it and who edits it is in
[Repository Files](../../reference/files.md).

- **Before pushing,** run [the checks](../guide.md#before-you-push).
- **Branches and pull requests** are in [Branches and
  Releases](../../../../scripts/docs/more/developing/releases.md).
- **Workflows** pin every action to a commit hash, with the tag in a
  comment. Jobs that run third-party actions get no secrets and
  `persist-credentials: false`. Signing and publishing are separate jobs
  that use only `actions/*`.
- **Docs** follow [Writing Docs](../../../../tests/docs/docs/more/developing/documentation/writing-docs.md).
- **Command verbs** come from one set, each pair never crossed:
  `create` and `delete` for what comes into and goes out of existence,
  `add` and `remove` for a connection between two things, `connect` and
  `disconnect`, `install` and `uninstall`, `start` and `stop`, `list`
  for every list and `show` for one thing. A box is created and
  deleted, never removed, and there are no aliases.
- **Everything is listed.** Whatever exists is in its kind's `list`,
  running or not and wherever it is, from `create` until `delete`, so
  nothing is lost or orphaned.
- **No ids.** Docs, the website, help text and code comments say what a
  thing is, never a letter-number id (A1, Z9) for it.
- **Docs describe what exists today.** They have no "yet", "later" or
  "Where it goes", and no roadmap. A known gap is stated
  as a fact. What's built is in the
  docs, and why is in their `decisions.md`.
- **Security and configuration docs** change with the code. A change to a
  limit, an auth check, what is stored or logged, or a setting updates the
  part's `security.md` (`docs/<part>/security.md`, or
  `docs/more/architecture/security.md` for what spans parts) and
  `core/cli/docs/more/architecture/files.md` in the same PR.
- **Logging.** Each component's security page describes what it logs
  and where. Keep credentials out of diagnostics, and update that page
  when the recorded information changes.
- **Lints.** Every package takes its workspace's lint levels with
  `[lints] workspace = true`. They're Clippy's default groups, and they
  refuse `dbg!`, `todo!`, `unimplemented!` and a suppression that says
  no reason.
- **Suppressions say why.** A lint is suppressed where it fires, with
  `#[expect(lint, reason = "...")]`, which fails once the lint stops
  firing. Only a lint that fires on some configurations alone, such as
  one OS or test builds, takes `#[allow(lint, reason = "...")]`.
- **Dependencies.** [Trusted, few, large and current](../../../../tests/repository/docs/more/developing/testing/repository-checks.md), crates and npm
  packages alike, development tools included:
  - Trusted: a vendor's official SDK, a package its own organization
    publishes, or a maintained one widely used: a crate with a million
    downloads or more, an npm package with 100,000 a week; never an
    unofficial client of a third-party service.
  - No advisory against a locked version, unless no release fixes it
    and it's listed with why; an unmaintained crate only where it reads
    no input from outside Link.
  - A heavy capability most runs don't need, such as reading PDFs, is
    a module of its own, so a build without it compiles none of it.
  - Fewer is better. Depend on major, well-maintained code bases, never
    a small, rarely maintained or bespoke crate; drop one of those
    outright and write what we used of it ourselves when that is small.
  - One version of each crate in a workspace, and the highest that
    allows. When a major dependency (`axum`) holds a shared crate back,
    pin that crate to its version; when a small one does, replace the
    small one.
  - A second version only where neither unifies a crate.
- **Licenses.** Link is MIT or Apache-2.0, so new dependencies must be
  permissively licensed.
- **Community files** in `.github/` only point at their pages:
  `docs/more/README.md`, `docs/more/code-of-conduct.md` and
  `docs/more/vulnerabilities.md`.
