//! Process input is captured here once, then parsed or forwarded as typed data.
use super::{Parsed, Program};
use std::ffi::OsString;

static INVOCATION: std::sync::OnceLock<Vec<OsString>> = std::sync::OnceLock::new();
static REPLAY: std::sync::OnceLock<Replay> = std::sync::OnceLock::new();

#[expect(
    clippy::disallowed_methods,
    reason = "the shared entry is the one process-argument reader"
)]
pub fn initialize() {
    let arguments = INVOCATION.get_or_init(|| std::env::args_os().collect());
    link_format::set_invocation(arguments);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Discovery {
    Description,
    Documentation,
    /// What a program of a release changes in an upgrade, or its
    /// changes made (`link_files::upgrade`).
    Upgrade(link_files::upgrade::Stage),
}

/// The request `lnk upgrade` makes of a release's program, followed by
/// its stage's word.
pub const UPGRADE: &str = "--link-upgrade";

pub fn discovery() -> Option<Discovery> {
    initialize();
    match INVOCATION.get()?.get(1..)? {
        [argument] if argument == "--link-describe" => Some(Discovery::Description),
        [argument] if argument == "--link-documentation" => Some(Discovery::Documentation),
        [argument, stage] if argument == UPGRADE => {
            link_files::upgrade::Stage::parse(stage.to_str()?).map(Discovery::Upgrade)
        }
        _ => None,
    }
}

pub fn current<T: clap::Parser>(
    program: &impl Program,
    adjust: impl Fn(clap::Command) -> clap::Command,
) -> Parsed<T> {
    try_current(program, adjust).unwrap_or_else(|error| error.exit())
}

pub fn try_current<T: clap::Parser>(
    program: &impl Program,
    adjust: impl Fn(clap::Command) -> clap::Command,
) -> Result<Parsed<T>, super::Refused> {
    initialize();
    let parsed = super::try_run(
        program,
        INVOCATION
            .get()
            .expect("initialized invocation")
            .iter()
            .cloned(),
        adjust,
    )?;
    let _ = REPLAY.set(parsed.replay.clone());
    Ok(parsed)
}

/// A parsed invocation, retaining runtime arguments and normalized field flags.
#[derive(Clone)]
pub struct Replay {
    /// The word `lnk` dispatches to this program by, which its own
    /// arguments never hold: `box` for `lnk-box`.
    namespace: Option<OsString>,
    arguments: Vec<OsString>,
    fields: Vec<OsString>,
    flags: Vec<clap::Arg>,
}

impl Replay {
    /// `command` built, `words` those of the command parsed.
    pub(super) fn new(
        command: &clap::Command,
        namespace: Option<&str>,
        words: &[String],
        arguments: Vec<OsString>,
        fields: Vec<OsString>,
    ) -> Self {
        let mut at = command;
        for word in words {
            match at.find_subcommand(word) {
                Some(sub) => at = sub,
                None => break,
            }
        }
        Self {
            namespace: namespace.map(OsString::from),
            arguments,
            fields,
            flags: at.get_arguments().cloned().collect(),
        }
    }

    /// Forward the invocation without one declared scalar long runtime flag.
    /// Values of other flags and everything after `--` stay untouched.
    pub fn without(&self, id: &str) -> anyhow::Result<Vec<OsString>> {
        let target = self
            .flags
            .iter()
            .find(|arg| arg.get_id() == id)
            .ok_or_else(|| anyhow::anyhow!("the parsed command declares no {id} argument"))?;
        anyhow::ensure!(
            target.get_long().is_some()
                && target.get_short().is_none()
                && target.get_action().takes_values()
                && target
                    .get_num_args()
                    .is_none_or(|range| range.max_values() == 1),
            "{id} is not a scalar long runtime flag without a short form"
        );
        let mut output = Vec::new();
        let mut arguments = self.arguments.iter().peekable();
        while let Some(argument) = arguments.next() {
            if argument == "--" {
                output.extend(self.fields.iter().cloned());
                output.push(argument.clone());
                output.extend(arguments.cloned());
                return Ok(output);
            }
            let (flag, inline) = self.flag(argument);
            let remove = flag.is_some_and(|flag| flag.get_id() == target.get_id());
            if !remove {
                output.push(argument.clone());
            }
            if let Some(flag) = flag
                && flag.get_action().takes_values()
            {
                let range = flag.get_num_args().unwrap_or(1.into());
                let mut values = usize::from(inline);
                while values < range.max_values() {
                    let Some(next) = arguments.peek() else {
                        break;
                    };
                    if next.as_os_str() == "--"
                        || (values >= range.min_values()
                            && !flag.is_allow_hyphen_values_set()
                            && next.to_string_lossy().starts_with('-'))
                    {
                        break;
                    }
                    let value = arguments.next().expect("peeked argument");
                    values += 1;
                    if !remove {
                        output.push(value.clone());
                    }
                }
            }
        }
        output.extend(self.fields.iter().cloned());
        Ok(output)
    }

    /// [`Replay::without`], as a person types it after `lnk`: the
    /// program's namespace first (`box create ...` for `lnk-box`), so
    /// another `lnk` runs the same command.
    pub fn typed_without(&self, id: &str) -> anyhow::Result<Vec<OsString>> {
        Ok(self
            .namespace
            .iter()
            .cloned()
            .chain(self.without(id)?)
            .collect())
    }

    fn flag(&self, argument: &OsString) -> (Option<&clap::Arg>, bool) {
        let Some(text) = argument.to_str() else {
            return (None, false);
        };
        if let Some(long) = text.strip_prefix("--") {
            let (long, inline) = long
                .split_once('=')
                .map_or((long, false), |(name, _)| (name, true));
            return (
                self.flags.iter().find(|arg| arg.get_long() == Some(long)),
                inline,
            );
        }
        if let Some(short) = text.strip_prefix('-') {
            let mut letters = short.chars();
            while let Some(letter) = letters.next() {
                let Some(flag) = self
                    .flags
                    .iter()
                    .find(|arg| arg.get_short() == Some(letter))
                else {
                    break;
                };
                if flag.get_action().takes_values() {
                    return (Some(flag), !letters.as_str().is_empty());
                }
            }
        }
        (None, false)
    }
}

/// Replay this process's parsed invocation while removing a runtime input.
pub fn replay_without(id: &str) -> anyhow::Result<Vec<OsString>> {
    REPLAY
        .get()
        .ok_or_else(|| anyhow::anyhow!("no invocation has been parsed"))?
        .without(id)
}

/// [`replay_without`], as a person types it after `lnk`, its namespace
/// first: for handing the command to another `lnk`.
pub fn replay_typed_without(id: &str) -> anyhow::Result<Vec<OsString>> {
    REPLAY
        .get()
        .ok_or_else(|| anyhow::anyhow!("no invocation has been parsed"))?
        .typed_without(id)
}

/// `command` built, as the entry hands it to [`Replay::new`].
#[cfg(test)]
fn built(mut command: clap::Command) -> clap::Command {
    command.build();
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};
    #[derive(Parser)]
    struct Cli {
        #[arg(long)]
        out: Option<String>,
        #[arg(short = 'n', long, allow_hyphen_values = true)]
        note: Option<String>,
        #[arg(short = 'v', long)]
        verbose: bool,
        #[arg(long, num_args = 2, allow_hyphen_values = true)]
        pair: Vec<String>,
        #[arg(last = true)]
        program: Vec<String>,
    }
    #[test]
    fn removing_a_flag_preserves_values_and_the_forwarded_program() {
        let arguments = [
            "--note",
            "--out",
            "--out=old",
            "--",
            "program",
            "--out",
            "theirs",
        ]
        .map(OsString::from)
        .to_vec();
        assert!(
            Cli::try_parse_from(
                std::iter::once(OsString::from("example")).chain(arguments.iter().cloned())
            )
            .is_ok()
        );
        let replay = Replay::new(
            &built(Cli::command()),
            None,
            &[],
            arguments,
            vec!["--spec.name=\"example\"".into()],
        );
        assert_eq!(
            replay.without("out").unwrap(),
            [
                "--note",
                "--out",
                "--spec.name=\"example\"",
                "--",
                "program",
                "--out",
                "theirs"
            ]
            .map(OsString::from)
        );
        assert!(replay.without("unknown").is_err());
    }
}

#[cfg(test)]
mod clustered {
    use super::*;
    use clap::{CommandFactory, Parser};
    #[test]
    fn clustered_options_and_multiple_values_keep_their_flag_shaped_data() {
        #[derive(Parser)]
        struct Cli {
            #[arg(long)]
            out: Option<String>,
            #[arg(short = 'v')]
            verbose: bool,
            #[arg(short = 'n', allow_hyphen_values = true)]
            note: Option<String>,
            #[arg(long, num_args = 2, allow_hyphen_values = true)]
            pair: Vec<String>,
        }
        let arguments = ["-vn", "--out", "--pair", "first", "--out", "--out=remove"]
            .map(OsString::from)
            .to_vec();
        Cli::try_parse_from(std::iter::once(OsString::from("example")).chain(arguments.clone()))
            .unwrap();
        let replay = Replay::new(&built(Cli::command()), None, &[], arguments, vec![]);
        assert_eq!(
            replay.without("out").unwrap(),
            ["-vn", "--out", "--pair", "first", "--out"].map(OsString::from)
        );
    }
}
