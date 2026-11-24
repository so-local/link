# Developing

> Developing Link, from Build to Test to Ship

Build a usable development installation, check a change and prepare a
bundle. Learn the runtime relationships in [Architecture](../architecture/README.md),
find where code belongs in [Repository Organization](../architecture/organization.md),
and look up terms in the [Developer Glossary](../architecture/glossary.md).

## Run a Development Agent

Start with Rust and the platform prerequisites in
[Development Environment](setup/agents.md). Build the CLI, then let its
agents selection build the required parts from the same checkout.
Clone `https://github.com/so-local/link` and run `cargo build -p link-cli` in the checkout. Then `./target/debug/lnk up agents`, `./target/debug/lnk agent start` and `./target/debug/lnk agent chat` start a development agent, its first start asking for a model and configuration.

### Update the Development Installation

Rebuild the CLI after source changes, then refresh the installed parts.
This reuses Cargo's build output and leaves missing parts uninstalled.
[Source Installation](setup/install.md#refresh-installed-modules) explains
selection, failure handling and how source installs differ from release installs. Run `cargo build -p link-cli`, then `./target/debug/lnk up --source . --stale`.

## Check a Change

Read the choices behind builds, tests, and releases in
[Development Decisions](decisions.md). Use [Troubleshooting](troubleshooting.md) when source
installations conflict or commands cannot find the expected programs.

### Select Checks

Select tests, format code, and run Clippy as the
[Developing Guide](guide.md#test) says, then run the
[before-push checks](guide.md#before-you-push). Capability requirements are in
[Test Environments](../../../packages/test-kit/docs/more/developing/testing/test-environments.md).
The [Testing Kit](../../../packages/test-kit/docs/more/developing/testing/testing-kit.md)
provides the shared fixtures and process helpers.

## Prepare a Bundle

[Link Bundle](../../../modules/tooling/bundle/docs/tooling/bundle/README.md)
turns a committed checkout into a signed, installable folder. Start with
the prerequisites, then choose a development, staging or production
bundle. Bundle's documentation covers target selection, signing and installation. Run `./target/debug/lnk bundle list`, then `./target/debug/lnk bundle create devbox`.

## Continuous Integration

The [checks workflow](../../../.github/workflows/checks.yml) runs the
repository's checks in its configured environments. Bundle's
[Build in CI](../../../modules/tooling/bundle/docs/tooling/bundle/README.md#build-in-ci)
explains building and sealing release parts. Publication is in
[Branches and Releases](../../../scripts/docs/more/developing/releases.md).

## Add to an Existing Interface

Add a harness, memory engine, connection, or cloud through its owning
interface, as [Write a Module](modules.md) explains. Find the owning API
references, grouped by types, files, runtime behavior, and interactions, in
[Shared Libraries](../../../packages/docs/more/libraries/README.md). Follow those references
when extending an existing caller instead of adding another implementation.

## Development Procedures

Prepare a checkout and its tools with [Set Up Development](setup/README.md).
Run the supported checks and fixtures with [Test a Change](testing/README.md).
Keep the reading tree and its claims current with
[Maintain Documentation](documentation/README.md). The
[Websites](../../../web/docs/more/developing/website/README.md)
guide covers the TypeScript applications and their builds.

<!-- docs-tree:start -->

## Docs

### Pages

- [Developing Guide](guide.md): Build `lnk` from source, test it, and ship a change to `main`
- [Write a Module](modules.md): Add a module, a harness, a memory engine, a connection or a cloud to Link, with its code in the module it belongs to
  - [Specifications and Command Lines](modules/specs.md): Specifications and command-line flags are two inputs to the same typed configuration
- [Maintain Documentation](documentation/README.md): Begin with the reader's task and verify each claim against the owning implementation
- [Set Up Development](setup/README.md): Prepare a checkout and its tools
- [Testing](testing/README.md): Local Link is tested by plain Cargo on your own machine, with no paid CI

### More

- [Decisions](decisions.md): Why the code and its tests work the way they do
- [Troubleshooting](troubleshooting.md): What to do when [installs of `lnk` mix](setup/install.md), a module won't install, or a pasted command misreads

<!-- docs-tree:end -->
