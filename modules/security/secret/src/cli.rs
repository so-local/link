//! `lnk secret`'s command line and its outputs, generated from the
//! module's declarations (`actions.toml` and `types.toml`).

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// Reads this process's command line: the command it names.
pub fn parse() -> Command {
    PROGRAM.parse::<Cli>().command.command
}

/// `lnk secret`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

#[cfg(test)]
mod tests {
    /// `lnk secret keeper create` takes `--yes`, as every command that may ask
    /// does, so a script runs it as it runs the rest.
    #[test]
    fn the_keeper_takes_yes() {
        let words = ["lnk-secret", "keeper", "create", "--yes"];
        let matches = super::command().try_get_matches_from(words);
        assert!(matches.is_ok(), "{matches:?}");
    }
}
