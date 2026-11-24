//! Mini Harness: a small model/tool loop, persistent runtime, and Link integration.

pub mod core;
mod link;
pub mod runtime;

pub use link::run;

/// Entry point for contract tests imitating another harness's ACP capabilities.
/// The fixture executable owns its environment controls, not the installed Mini.
#[cfg(feature = "fixture")]
pub fn fixture(
    model: link_harness_contract::harness::Model,
    tools_from_env: bool,
    commands: Vec<String>,
    state: bool,
) -> anyhow::Result<()> {
    let mut options = link::acp::Options::new(model);
    options.tools_from_env = tools_from_env;
    options.commands = commands;
    options.state = state;
    options.media_descriptions = true;
    link::acp::serve(options)
}

/// The test-only adapter reuses Mini's integration with descriptive fake media.
#[cfg(feature = "fixture")]
pub fn fixture_adapter(
    out: &mut link_format::Out,
    command: cli::Command,
) -> Result<link_format::Done, link_format::Report> {
    link::run_command(out, true, command)
}

#[cfg(test)]
link_test_kit::program!(unit);

/// `lnk-mini-harness`'s command line, generated from its declarations:
/// the harness protocol's commands its contract declares
/// (`actions.toml`), whose shapes are the contract's
/// (`link_harness_contract::declared`).
pub mod cli {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));

    /// The command this run is given.
    pub fn parse() -> Command {
        let parsed: link_spec::Parsed<Cli> = PROGRAM.parse();
        parsed.command.command
    }

    /// `lnk-mini-harness`'s whole command line, as clap sees it.
    pub fn command() -> clap::Command {
        <Cli as clap::CommandFactory>::command()
    }
}
