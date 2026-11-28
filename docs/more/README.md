# More

> More Ways to Build Local Link and Help

**Local Link is open source.** Here's how to build it and help. Anything
you contribute is [licensed as Local Link is](https://www.local.link/license), under both MIT and
Apache with no additional terms, unless you say otherwise. Each product name has one meaning, which [Product Names](architecture/names.md)
explains, so a change says the one it means. Read the
[Developer Glossary](architecture/glossary.md) to meet the programs and their
installable parts before their Rust implementation.

<!-- quickstart -->

## Understand Local Link

Read what guides Local Link in [Our Values](values.md).
See how the parts cooperate in [Architecture](architecture/README.md). Find the
source folders and who owns them in [Repository Organization](architecture/organization.md).
Look up the products those pages mention in [Product Names](architecture/names.md).

## Build and Contribute

Find where we'd love your help in [Contribute Here](contribute-here.md), then [send a change](#send-a-change) or [get help](#get-help). Everyone who takes
part follows the [Code of Conduct](code-of-conduct.md). Report a security
problem privately through [Report a Vulnerability](vulnerabilities.md).
Build a usable local agent, then move on to checks, bundles and continuous
integration in [Developing](developing/README.md). Every program in Local Link is written in
Rust, with native executables: [Built in Rust](architecture/rust.md).

## Send a Change

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

## Get Help

Start with the docs, and open an issue for a bug or an idea.

- **Using Link:** start with [Install](modules/install.md), then the
  page of the part you use.
- **Something broken:** run [`lnk issue create`](../../core/cli/docs/tooling/issue.md#report-a-bug) after the command that failed.
  It prints the [bug
  report](https://github.com/so-local/link/issues/new?template=bug_report.yml)'s
  fields, scrubbed of your names, paths, addresses and anything shaped
  like a secret, to read and paste; `lnk issue create --open` fills them in on
  the form. Link sends nothing itself.
- **An idea:** open a [feature
  request](https://github.com/so-local/link/issues/new?template=feature_request.yml).
- **A security problem:** never an issue. See [Report a
  Vulnerability](vulnerabilities.md).

## Shared Libraries

Install, inspect, and upgrade the parts you use with
[Link Modules](modules/README.md) before working on their implementations.
Find APIs grouped by what a caller needs (types, files, runtime behavior, or
user interaction) in [Shared Libraries](../../packages/docs/more/libraries/README.md). Choose an
existing owner before adding another implementation.

## Shared Inputs and Locations

Find the command catalogue, specification rules, declared environment inputs,
file locations, and repository metadata in [Reference](reference/README.md).
Use Reference for facts shared across features, then follow each feature's
guide for tasks and behavior.

<!-- docs-tree:start -->

## Docs

- [Our Values](values.md): Our values say how we build
- [Architecture](architecture/README.md): Local Link programs are assembled from a small core and independently installed parts
- [Code of Conduct](code-of-conduct.md): Everyone who takes part in Local Link keeps to the Contributor Covenant below, and a breach can be reported to so@local.link
- [Developing](developing/README.md): Build a usable development installation, check a change and prepare a bundle
- [Link Modules](modules/README.md): Local Link is modular, so you install only the parts you use
- [Reference](reference/README.md): Find the inputs and interfaces that apply across Local Link's programs
- [Report a Vulnerability](vulnerabilities.md): Report it privately, with Report a vulnerability on the repository's Security tab or by mail to so@local.link
- [Contribute Here](contribute-here.md): Here's where we'd love your help

<!-- docs-tree:end -->
