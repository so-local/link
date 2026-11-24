//! `lnk sandbox`'s command line and its outputs, generated from the
//! module's declarations (`actions.toml`), over the spec and shapes its
//! contract owns (`types.toml`). What a sandbox may touch is its spec's
//! fields, given as a file (`--spec`) or as the field flags generated
//! from the spec's shape (`--files.write`).

include!(concat!(env!("OUT_DIR"), "/declared.rs"));

/// What this process was run for.
pub enum Invoked {
    /// A browser handing over a page to open (`BROWSER=lnk-sandbox`): a
    /// machine protocol, its one argument the page.
    Page(String),
    /// A command and the field flags of the spec it binds.
    Command(Box<link_spec::Parsed<Cli>>),
}

/// Reads this process's command line: a browser's page, or a command.
pub fn parse() -> Invoked {
    let program = Sandbox {
        declared: declared(),
        page: std::sync::OnceLock::new(),
    };
    match link_spec::entry::try_current::<Cli>(&program, |command| command) {
        Ok(parsed) => Invoked::Command(Box::new(parsed)),
        Err(refused) => match program.page.into_inner() {
            Some(page) => Invoked::Page(page),
            None => refused.exit(),
        },
    }
}

/// The sandbox's program to the entry: a module's, and before parsing,
/// a browser's page noted, which no command parses.
struct Sandbox {
    declared: link_core::help::Declared,
    page: std::sync::OnceLock<String>,
}

impl link_spec::Program for Sandbox {
    fn before(&self, args: &[std::ffi::OsString]) {
        if let [_, page] = args
            && let Some(page) = page.to_str().filter(|p| crate::bridges::open::is_page(p))
        {
            let _ = self.page.set(page.to_string());
        }
        self.declared.before(args);
    }

    fn styled(&self, command: clap::Command) -> clap::Command {
        self.declared.styled(command)
    }

    fn helped(&self, command: clap::Command) -> clap::Command {
        self.declared.helped(command)
    }

    fn history(&self) -> link_spec::history::History {
        self.declared.history()
    }

    fn release(&self) -> Option<fn() -> anyhow::Result<link_spec::history::Wire>> {
        self.declared.release()
    }

    fn binding(&self, words: &[String]) -> Option<link_spec::Binding> {
        self.declared.binding(words)
    }

    fn namespace(&self) -> Option<&str> {
        Some(NAMESPACE)
    }
}

/// Reads `args`, the program's name first, as [`parse`] reads this
/// process's: a command line this program wrote for itself.
pub fn parse_from(
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> anyhow::Result<link_spec::Parsed<Cli>> {
    link_spec::try_run::<Cli>(&declared(), args, |command| command)
        .map_err(|refused| anyhow::anyhow!("{refused}"))
}

fn declared() -> link_core::help::Declared {
    PROGRAM
}

/// `lnk sandbox`'s whole command line, as clap sees it.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

/// A spec's keys that mean something on macOS alone: their flags are
/// listed nowhere else.
const MACOS_ONLY: &[&str] = &["commands.developer_tools"];

/// A spec's sections as `lnk sandbox permission list` lists them: each
/// section, its field flags on this system (each its key path,
/// `--files.write`), and what it opens, from the spec's type.
pub fn sections() -> Vec<(String, String, String)> {
    link_sandbox_client::Spec::sections()
        .into_iter()
        .filter_map(|section| {
            let name = section.name?;
            let given: Vec<String> = section
                .fields
                .iter()
                .map(|(key, _)| format!("{name}.{key}"))
                .filter(|path| cfg!(target_os = "macos") || !MACOS_ONLY.contains(&path.as_str()))
                .map(|path| format!("--{path}"))
                .collect();
            Some((name, given.join(", "), section.about))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    /// A flag that means nothing here isn't listed: Xcode's tools are the
    /// Mac's.
    #[test]
    fn the_mac_s_flags_are_listed_only_there() {
        let commands = super::sections()
            .into_iter()
            .find(|(section, _, _)| section == "commands")
            .expect("a commands section");
        assert!(commands.1.contains("--commands.allow"), "{}", commands.1);
        assert_eq!(
            commands.1.contains("--commands.developer_tools"),
            cfg!(target_os = "macos"),
            "{}",
            commands.1
        );
    }

    /// `lnk sandbox workers --install` takes `--yes`, as every command
    /// that may ask does, so a script runs it as it runs the rest.
    #[test]
    fn installing_workers_takes_yes() {
        let parsed =
            super::parse_from(["lnk-sandbox", "workers", "--install", "--yes"].map(Into::into))
                .unwrap();
        let super::Command::SandboxWorkers(workers) = parsed.command.command else {
            panic!("expected workers");
        };
        assert!(workers.install && workers.yes);
    }

    /// The command after `--` is the command, never a flag of the run.
    #[test]
    fn positional_commands_and_allowed_command_option_are_distinct() {
        let parsed =
            super::parse_from(["lnk-sandbox", "run", "--", "echo", "hello"].map(Into::into))
                .unwrap();
        let super::Command::SandboxRun(run) = parsed.command.command else {
            panic!("expected run");
        };
        assert_eq!(run.commands, ["echo", "hello"]);
    }

    /// A stage's program is taken only after `--`, its flags handed on
    /// whole, and a mistyped flag before `--` refused, never run.
    #[test]
    fn a_stages_program_is_only_what_follows_the_dashes() {
        let args = |words: &[&str]| {
            super::parse_from(
                ["lnk-sandbox", "placeholders", "--dir", "/s"]
                    .iter()
                    .chain(words)
                    .map(Into::into),
            )
        };
        let parsed = args(&[
            "--wanted",
            "file",
            "--wanted",
            "/p",
            "--wanted",
            ".env",
            "--",
            "bwrap",
            "--unknown-to-link",
        ])
        .unwrap();
        let super::Command::SandboxPlaceholders(given) = parsed.command.command else {
            panic!("expected placeholders");
        };
        assert_eq!(given.wanted, ["file", "/p", ".env"]);
        assert_eq!(given.command, ["bwrap", "--unknown-to-link"]);
        assert!(args(&["--unknown", "--", "sh"]).is_err());
        assert!(args(&["sh"]).is_err());
    }
}
