//! `lnk storage`'s command line and its outputs, generated from the
//! module's declarations (`actions.toml` and `types.toml`).

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// Reads this process's command line.
pub fn parse() -> Command {
    PROGRAM.parse::<Cli>().command.command
}

/// `lnk storage`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

/// What `restore` runs itself with to get an owner's files back: `pull
/// <folder>`, its namespace word left out, as its program never sees it.
pub fn pull_again(into: &std::path::Path) -> [&std::ffi::OsStr; 2] {
    ["pull".as_ref(), into.as_os_str()]
}

#[cfg(test)]
mod tests {
    /// The program reads restore's re-run as `lnk storage pull <folder>`.
    #[test]
    fn restore_runs_itself_as_a_pull_of_the_folder() {
        let into = std::path::Path::new("/home/me/Link/Agents/main");
        let argv = std::iter::once("lnk".as_ref()).chain(super::pull_again(into));
        let matches = super::command().try_get_matches_from(argv).unwrap();
        let (word, pull) = matches.subcommand().unwrap();
        assert_eq!(word, "pull");
        let paths: Vec<&String> = pull.get_many("paths").unwrap().collect();
        assert_eq!(paths, ["/home/me/Link/Agents/main"]);
    }
}
