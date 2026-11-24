# More Guide

**Send a change to Local Link, or get help using it.** What a
contribution is licensed under, and where each part of the work is
explained, is on [More](README.md).

## Developing

Build, run, test and ship a change to Local Link:
[Developing](developing/README.md). Every program in Local Link is
written in Rust, with native executables: [Built
in Rust](architecture/rust.md).

## Find Something to Do

Every place where we'd love a contribution is on [Contribute Here](contribute-here.md),
each item linked to the feature it improves, with what it does today and
what your change would add.

## Send a change

For anything bigger than a fix, open an issue before writing it.

- **Where things go:** [Architecture](architecture/README.md) has the
  layout, the dependency rules (a test enforces them) and a table of
  where new code goes. [Conventions](developing/setup/conventions.md) has the
  rules every change follows.
- **Building, testing and the checks to run before pushing:**
  [Developing](developing/guide.md#before-you-push), and [The
  Website](../../web/docs/more/developing/website/README.md) for
  the website.
- **Branches and pull requests:** [Branches and
  Releases](../../scripts/docs/more/developing/releases.md).
- **New dependencies** must be permissively licensed.
  `tests/repository/tests/licenses.rs` lists what's accepted.
- **Conduct** follows the [Contributor Covenant](code-of-conduct.md).

## Getting help

Start with the docs, and open an issue for a bug or an idea.

- **Using Link:** start with [Install](modules/install.md), then the
  page of the part you use.
- **Something broken:** run [`lnk issue create`](modules/guide.md#get-help) after the command that failed.
  It prints the [bug
  report](https://github.com/so-local/link/issues/new?template=bug_report.yml)'s
  fields, scrubbed of your names, paths, addresses and anything shaped
  like a secret, to read and paste; `lnk issue create --open` fills them in on
  the form. Link sends nothing itself.
- **An idea:** open a [feature
  request](https://github.com/so-local/link/issues/new?template=feature_request.yml).
- **A security problem:** never an issue. See [Report a
  Vulnerability](vulnerabilities.md).
