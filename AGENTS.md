# Local Link

Local Link runs agents on your own computers and clouds, inside
sandboxes you control. This repository holds `lnk`, its modules and
packages, adapters for other products, and the documentation website
(`web/`). This file helps an agent explore it for a contributor: learn
how it fits together, answer questions from its docs, find work, and
send a change.

## Learn How It Fits Together

Start at the [home page](docs/guide.md) for what Local Link does, then
[Architecture](docs/more/architecture/README.md) for how `lnk` runs a
module for each part. [Repository Organization](docs/more/architecture/organization.md)
says which folder owns what, and the
[Developer Glossary](docs/more/architecture/glossary.md) names the
programs and parts before their Rust.

## Answer Questions from the Docs

Every fact lives once, in the `docs/` folder of the part that owns it,
and each page is a page of the website. A part's `README.md` lists its
pages. Search a part's `docs/` before its source, and follow a page's
links for the details it leaves to another page.

## Report a Problem

After a command fails, `lnk issue create` prints a bug report scrubbed
of names, paths, addresses and anything shaped like a secret, ready to
read and paste. [Getting Help](docs/more/README.md#get-help) has the
issue forms, and a security problem never goes in an issue: see
[Report a Vulnerability](docs/more/vulnerabilities.md).

## Find Something to Do

[Contribute Here](docs/more/contribute-here.md) lists every place a
contribution is welcome, each linked to the feature it improves. For
anything bigger than a fix, open an issue before writing it.

## Build and Check

Plain `cargo build` and `cargo test` build only the core: select another
package with `-p`, or every member with `--workspace`. Tests run
programs from `target/debug`, so `cargo build --workspace --bins` comes
before `cargo test --workspace`. The checks a change passes before it's
pushed are in [Before you push](docs/more/developing/guide.md#before-you-push),
and what the machine needs is in
[Development Environment](docs/more/developing/setup/agents.md).

## Send a Change

Docs change with the code, in the same pull request, following
[Writing Docs](tests/docs/docs/more/developing/documentation/writing-docs.md).
The other rules every change follows are in
[Conventions](docs/more/developing/setup/conventions.md), and
[Send a Change](docs/more/README.md#send-a-change) covers branches and
pull requests. Describe only contents present in this repository. Omit
absent folders, components, and capabilities rather than explaining
their absence. Use Oxford commas in prose lists of three or more items.
