//! `lnk-file-store`'s command line, generated from its declarations: the store
//! protocol's commands its contract declares (`actions.toml`), whose
//! shapes are the contract's (`link_store_contract::declared`).

pub mod cli {
    include!(concat!(env!("OUT_DIR"), "/declared.rs"));

    /// The store protocol's command this run is given. A store keeps
    /// nothing of its own to remove, so core never asks it to uninstall.
    pub fn operation() -> anyhow::Result<Option<link_store_contract::store::Operation>> {
        use link_store_contract::store::Operation;
        let parsed: link_spec::Parsed<Cli> = PROGRAM.parse();
        Ok(Operation::of(&parsed.words))
    }

    /// `lnk-file-store`'s whole command line, as clap sees it.
    pub fn command() -> clap::Command {
        <Cli as clap::CommandFactory>::command()
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
