//! `lnk-deepseek`'s command line, generated from its declarations: the
//! harness protocol's commands its contract declares (`actions.toml`),
//! whose shapes are the contract's (`link_harness_contract::declared`).

pub mod cli {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));

    /// The command this run is given.
    pub fn parse() -> Command {
        let parsed: link_spec::Parsed<Cli> = PROGRAM.parse();
        parsed.command.command
    }

    /// `lnk-deepseek`'s whole command line, as clap sees it.
    pub fn command() -> clap::Command {
        <Cli as clap::CommandFactory>::command()
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
