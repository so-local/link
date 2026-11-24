//! `lnk-run-tool`'s command line and its outputs, generated from the
//! module's declarations (`actions.toml` and `types.toml`). Link runs
//! every command, never a person.

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// Reads this process's command line.
pub fn parse() -> link_spec::Parsed<Cli> {
    PROGRAM.parse()
}

/// `lnk-run-tool`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, FromArgMatches};

    /// `serve`'s line, read as this program reads its own.
    fn serve(more: &[&str]) -> Result<super::RunToolServeArgs, clap::Error> {
        let line = [
            "lnk-run-tool",
            "serve",
            "--files",
            "/f",
            "--output-bytes",
            "16384",
            "--programs-path",
            "/bin",
            "--timeout",
            "108s",
            "--program",
            "sh",
        ]
        .iter()
        .chain(more)
        .chain(&["--", "lnk"]);
        let matches = super::Cli::command().try_get_matches_from(line)?;
        match super::Cli::from_arg_matches(&matches)?.command {
            super::Command::RunToolServe(serve) => Ok(serve),
            _ => panic!("not serve"),
        }
    }

    /// `--with-env` is a switch: the context is in `LNK_RUN_CONTEXT`
    /// alone, so a variable's name after it is refused, and a timeout
    /// is read as a duration.
    #[test]
    fn its_context_is_one_variable_and_its_timeout_a_duration() {
        let read = serve(&["--with-env"]).unwrap();
        assert!(read.with_env);
        assert_eq!(read.timeout.get(), std::time::Duration::from_secs(108));
        assert!(!serve(&[]).unwrap().with_env);
        assert!(serve(&["--with-env", "OTHER"]).is_err());
    }
}
