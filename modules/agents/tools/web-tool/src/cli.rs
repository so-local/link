//! `lnk-web-tool`'s command line, generated from the module's
//! declarations (`actions.toml` and `types.toml`) over the outputs its
//! contract generates, so the tool and every module naming them read one
//! type. A harness runs every command, never a person.

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

pub use link_web_tool_contract::{Hit, Page, Results};

/// Reads this process's command line.
pub fn parse() -> link_spec::Parsed<Cli> {
    PROGRAM.parse()
}

/// `lnk-web-tool`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, FromArgMatches};
    use link_web_tool_contract::WebToolServeArgs;

    /// `serve`'s line, as the agent module builds it from the contract,
    /// reads back through this program's parser as it was built.
    #[test]
    fn the_serve_line_the_contract_builds_reads_back() {
        let built = WebToolServeArgs {
            private_host: vec!["localhost:8080".into()],
        };
        let line = std::iter::once("lnk-web-tool".to_string()).chain(built.argv());
        let matches = super::Cli::command().try_get_matches_from(line).unwrap();
        let super::Command::WebToolServe(read) =
            super::Cli::from_arg_matches(&matches).unwrap().command
        else {
            panic!("not serve");
        };
        assert_eq!(read.private_host, built.private_host);
        assert_eq!(
            WebToolServeArgs {
                private_host: Vec::new()
            }
            .argv(),
            ["serve"]
        );
    }
}
