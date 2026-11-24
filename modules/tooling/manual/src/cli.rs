//! `lnk manual`'s command line and its output, generated from the module's
//! declarations (`actions.toml` and `types.toml`).

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// Reads this process's command line.
pub fn parse() -> Command {
    PROGRAM.parse::<Cli>().command.command
}

/// `lnk manual`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A topic's commands, one command's reference and its variables are
    /// one listing each: two at once are refused, and each needs a topic.
    #[test]
    fn listing_and_reading_a_command_are_distinct_inputs() {
        let parse = |args: &[&str]| command().try_get_matches_from(args);
        assert!(parse(&["lnk", "compute/box", "--commands"]).is_ok());
        assert!(parse(&["lnk", "compute/box", "--command", "box create"]).is_ok());
        assert!(parse(&["lnk", "compute/box", "--commands", "box create"]).is_err());
        assert!(
            parse(&[
                "lnk",
                "compute/box",
                "--commands",
                "--command",
                "box create"
            ])
            .is_err()
        );
        assert!(parse(&["lnk", "show", "compute/box", "--commands"]).is_ok());
        for flag in [
            &["--commands"][..],
            &["--command", "box create"],
            &["--variables"],
        ] {
            let words: Vec<&str> = ["lnk"].iter().chain(flag).copied().collect();
            assert!(parse(&words).is_err(), "{flag:?} without a topic");
        }
    }
}
