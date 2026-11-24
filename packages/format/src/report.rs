//! A report: an error, a warning or a refusal, for its two readers, the
//! person now and whoever reads the issue they paste it into.

use serde::{Deserialize, Serialize};

use crate::{Command, Next, Phrase, Product, Prose, Sentence, Tail, Trace};

/// A report, boxed: every command's error is one pointer wide.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Report(Box<Body>);

/// A report's fields, reached through [`Report`].
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Body {
    pub kind: Kind,
    pub headline: Sentence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<Prose>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tail: Option<Tail>,
    pub next: Next,
    pub exit: Exit,
    /// Where it went wrong: each step up the chain with its place in the
    /// code, the report's own place first.
    pub trace: Trace,
    /// Made from an `anyhow::Error` by `?`, before its command wrote a
    /// report of its own: what the guard counts down.
    #[serde(skip)]
    pub adopted: bool,
    /// Link's own fault, not the person's or another service's: a panic,
    /// or what Link never meant to reach.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bug: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Error,
    Warning,
    /// A no: from whom, so a script and a person tell it from a failure.
    Refusal(Refuser),
}

/// Who refused.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Refuser {
    Sandbox,
    /// A spec, by its path.
    Spec(crate::Path),
    /// A policy, by its name.
    Policy(String),
    /// Another service: Telegram, a cloud.
    Service(Product),
    /// The person, at a question.
    Person,
}

impl Serialize for crate::Path {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.display().to_string())
    }
}

/// How a program exits: one list, each code one meaning, the public page
/// of exit codes checked against it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub enum Exit {
    Success,
    Failure,
    /// The command line was wrong, or a question had no one to answer it.
    Usage,
    Refused,
    /// The module a command needs isn't installed.
    NotInstalled,
    /// The sandbox couldn't run the program.
    SandboxCantRun,
    /// The sandbox couldn't find the program.
    SandboxNotFound,
    /// The person pressed Ctrl-C.
    Interrupted,
    /// A module's own, declared in [`MODULE_CODES`].
    Module(u8),
}

/// Each module's own codes, with their meanings: two modules can't give
/// one number two meanings, since both are here.
pub const MODULE_CODES: &[(u8, &str, &str)] = &[
    (10, "connections", "a connection's harness isn't linked"),
    (
        11,
        "connections",
        "a connection's service refused its token",
    ),
    (12, "connections", "a connection's service refused a reply"),
    (13, "connections", "a webhook was refused"),
];

impl Exit {
    pub fn code(self) -> u8 {
        match self {
            Exit::Success => 0,
            Exit::Failure => 1,
            Exit::Usage => 2,
            Exit::Refused => 3,
            Exit::NotInstalled => 4,
            Exit::SandboxCantRun => 126,
            Exit::SandboxNotFound => 127,
            Exit::Interrupted => 130,
            Exit::Module(code) => code,
        }
    }

    /// What a code means, for the exit codes' page.
    pub fn meaning(self) -> &'static str {
        match self {
            Exit::Success => "it did what it's for",
            Exit::Failure => "it failed",
            Exit::Usage => "the command line was wrong, or a question had no answer",
            Exit::Refused => "a sandbox, specification, policy, or service refused it",
            Exit::NotInstalled => "the module it needs isn't installed",
            Exit::SandboxCantRun => "the sandbox couldn't run the program",
            Exit::SandboxNotFound => "the sandbox couldn't find the program",
            Exit::Interrupted => "it was stopped with Ctrl-C",
            Exit::Module(code) => MODULE_CODES
                .iter()
                .find(|(c, ..)| *c == code)
                .map_or("a module's own code", |(_, _, m)| m),
        }
    }

    /// Every code but a module's, then the modules'.
    pub fn all() -> Vec<Exit> {
        let mut all = vec![
            Exit::Success,
            Exit::Failure,
            Exit::Usage,
            Exit::Refused,
            Exit::NotInstalled,
            Exit::SandboxCantRun,
            Exit::SandboxNotFound,
            Exit::Interrupted,
        ];
        all.extend(MODULE_CODES.iter().map(|(c, ..)| Exit::Module(*c)));
        all
    }
}

#[cfg(test)]
mod exit_codes {
    use super::Exit;

    /// The exit codes' page lists every code with its meaning, in order,
    /// and nothing else.
    #[test]
    fn the_page_lists_every_code() {
        let page = include_str!("../docs/more/modules/exit-codes.md");
        let mut codes = Exit::all();
        codes.sort_by_key(|c| c.code());
        let want: Vec<String> = codes
            .iter()
            .map(|c| format!("| {} | {} |", c.code(), c.meaning()))
            .collect();
        let rows: Vec<&str> = page
            .lines()
            .filter(|l| {
                l.starts_with("| ")
                    && l.split('|')
                        .nth(1)
                        .is_some_and(|c| c.trim().parse::<u8>().is_ok())
            })
            .collect();
        assert_eq!(
            rows,
            want,
            "the page's table should be:\n{}",
            want.join("\n")
        );
    }
}

impl Serialize for Exit {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(self.code())
    }
}

impl Report {
    #[track_caller]
    fn new(kind: Kind, headline: Sentence, exit: Exit) -> Self {
        Report(Box::new(Body {
            kind,
            headline,
            why: None,
            tail: None,
            next: Next::default(),
            exit,
            trace: Trace::here(),
            adopted: false,
            bug: false,
        }))
    }

    /// What didn't happen, with its subject: `Couldn't start my-otter.`
    #[track_caller]
    pub fn error(headline: Sentence) -> Self {
        Report::new(Kind::Error, headline, Exit::Failure)
    }

    /// Link's own fault, what it never meant to reach: the one kind of
    /// report that offers `lnk issue create`.
    #[track_caller]
    pub fn bug(headline: Sentence) -> Self {
        let mut r = Report::new(Kind::Error, headline, Exit::Failure);
        r.bug = true;
        r
    }

    /// Whether it's Link's own fault, so worth reporting: one made a
    /// bug, or an error no command wrote a report for, failing as only
    /// an unexpected one does. A refusal, a usage error and a report a
    /// command wrote end on their Next instead.
    pub fn is_bug(&self) -> bool {
        matches!(self.kind, Kind::Error)
            && (self.bug || (self.adopted && self.exit == Exit::Failure))
    }

    /// A command line `program` doesn't answer (a flag missing, unknown
    /// or with no value): what it wasn't given, and its `--help` next.
    /// What a [`Usage`] error becomes.
    #[track_caller]
    pub fn usage(program: &str) -> Self {
        let program = crate::Product::new(program);
        Report::error(crate::sentence!(
            "{} wasn't given what it needs.",
            program.clone()
        ))
        .why(crate::prose!("A flag is missing, unknown or has no value."))
        .next(
            Command::typed([program.0.clone(), "--help".into()]),
            crate::phrase!("see what it answers"),
        )
        .exit(Exit::Usage)
    }

    /// A fact and its cost; the command goes on.
    #[track_caller]
    pub fn warning(headline: Sentence) -> Self {
        Report::new(Kind::Warning, headline, Exit::Success)
    }

    /// Who refused, and what: `The sandbox refused my-otter's write to ~/.ssh.`
    #[track_caller]
    pub fn refusal(by: Refuser, headline: Sentence) -> Self {
        Report::new(Kind::Refusal(by), headline, Exit::Refused)
    }

    pub fn why(mut self, why: impl Into<Prose>) -> Self {
        self.why = Some(why.into());
        self
    }

    pub fn tail(mut self, tail: Tail) -> Self {
        self.tail = Some(tail);
        self
    }

    #[track_caller]
    pub fn next(mut self, command: Command, why: impl Into<Phrase>) -> Self {
        self.next.push(command, why);
        self
    }

    pub fn exit(mut self, exit: Exit) -> Self {
        self.exit = exit;
        self
    }

    /// The error that caused it: its chain joins the trace.
    pub fn cause(mut self, err: anyhow::Error) -> Self {
        self.trace.extend(Trace::of(&err));
        self
    }

    /// The trace of another program's report, read from its `--json`,
    /// under this one.
    pub fn under(mut self, inner: Trace) -> Self {
        self.trace.extend(inner);
        self
    }
}

impl std::ops::Deref for Report {
    type Target = Body;
    fn deref(&self) -> &Body {
        &self.0
    }
}

impl std::ops::DerefMut for Report {
    fn deref_mut(&mut self) -> &mut Body {
        &mut self.0
    }
}

/// An error a command didn't write a report for: its first cause, as a
/// sentence, for a headline, and the whole chain as the trace. `?` makes
/// one, so a module moves without rewriting every `bail!` at once.
impl From<anyhow::Error> for Report {
    #[track_caller]
    fn from(err: anyhow::Error) -> Self {
        // Its first line the headline, any more its tail, as it read.
        let message = err.to_string();
        let mut lines = message.lines();
        let mut report = Report::error(Sentence::adopt(lines.next().unwrap_or_default()));
        let more: Vec<crate::Foreign> = lines
            .filter(|l| !l.trim().is_empty())
            .map(crate::Foreign::new)
            .collect();
        if !more.is_empty() {
            report.tail = Some(Tail {
                lines: more,
                log: None,
                empty: None,
            });
        }
        // The causes under it, so nothing a person read before is lost: a
        // line each the why, and one of several lines (a parser's, with
        // its pointer) the tail, as it was written.
        let (long, short): (Vec<String>, Vec<String>) = err
            .chain()
            .skip(1)
            .map(|c| c.to_string())
            .partition(|c| c.trim().contains('\n'));
        if !short.is_empty() {
            report.why = Some(Prose::from(Sentence::adopt(&short.join(": "))));
        }
        let lines = long
            .iter()
            .flat_map(|c| c.lines())
            .filter(|l| !l.trim().is_empty())
            .map(crate::Foreign::new);
        let tail = report.tail.get_or_insert(Tail {
            lines: Vec::new(),
            log: None,
            empty: None,
        });
        tail.lines.extend(lines);
        if tail.lines.is_empty() {
            report.tail = None;
        }
        report.trace = Trace::here();
        report.trace.extend(Trace::of(&err));
        if err.is::<crate::NotInstalled>() {
            report.exit = Exit::NotInstalled;
        }
        if err.chain().any(|c| c.is::<Refused>()) {
            report.exit = Exit::Usage;
        }
        report.adopted = true;
        report
    }
}

/// A command line a program doesn't answer: `bail!(Usage)` or
/// `.context(Usage)` where it reads its arguments, and
/// [`Report::usage`] says so where it ends.
#[derive(Debug)]
pub struct Usage;

impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the command line isn't one it answers")
    }
}

impl std::error::Error for Usage {}

/// `lnk <group>` refused: its module isn't installed. Its text says how
/// to install it; a report made from it exits 4.
#[derive(Debug)]
pub struct NotInstalled(pub String);

impl std::fmt::Display for NotInstalled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NotInstalled {}

/// What a person gave, refused: a spec file or a flag that doesn't read.
/// It reads as its cause did, line for line, and a report made from it
/// is a usage error, never a fault of Link's to report ([`refused`]).
#[derive(Debug)]
pub struct Refused(pub anyhow::Error);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Refused {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

/// `err`, marked as [`Refused`]: what the person gave, not Link.
pub fn refused(err: anyhow::Error) -> anyhow::Error {
    match err.chain().any(|c| c.is::<Refused>()) {
        true => err,
        false => anyhow::Error::new(Refused(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A refusal of what a person gave ends as a usage error, its words
    /// and causes as they were, with no `lnk issue create` to report it.
    #[test]
    fn a_refused_input_is_a_usage_error_with_its_causes() {
        let err = anyhow::anyhow!("unknown field `cpu`").context("in box.toml");
        let report = Report::from(refused(err));
        assert_eq!(report.exit, Exit::Usage);
        assert!(!report.is_bug());
        let said = format!("{report:?}");
        assert!(
            said.contains("in box.toml") && said.contains("unknown field"),
            "{said}"
        );
        // The loosening: an error of Link's own stays one to report.
        assert!(Report::from(anyhow::anyhow!("a socket closed")).is_bug());
    }

    #[test]
    fn each_code_has_one_meaning() {
        let all = Exit::all();
        let mut codes: Vec<u8> = all.iter().map(|e| e.code()).collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), all.len());
        assert!(all.iter().all(|e| !e.meaning().is_empty()));
    }

    #[test]
    fn a_usage_error_names_its_help_and_exits_2() {
        let err = anyhow::Error::from(Usage);
        assert!(err.is::<Usage>());
        let r = Report::usage("lnk-memory-tool");
        assert_eq!(
            r.headline.to_string(),
            "lnk-memory-tool wasn't given what it needs."
        );
        assert_eq!(r.next.0[0].command.text(), "lnk-memory-tool --help");
        assert_eq!(r.exit, Exit::Usage);
        assert!(!r.is_bug());
    }

    #[test]
    fn question_mark_adopts_an_error() {
        fn fails() -> anyhow::Result<()> {
            anyhow::bail!("no box named \"qa\"")
        }
        fn command() -> Result<(), Report> {
            fails()?;
            Ok(())
        }
        let report = command().unwrap_err();
        let parsed =
            anyhow::anyhow!("unknown field `host`\n  |\n2 | host = 1").context("in spec.toml");
        let parsed = Report::from(parsed);
        assert_eq!(parsed.headline.to_string(), "in spec.toml.");
        assert_eq!(
            parsed.tail.as_ref().unwrap().lines[0].as_str(),
            "unknown field `host`"
        );
        assert!(parsed.why.is_none());
        assert!(report.adopted);
        assert_eq!(report.headline.to_string(), "no box named \"qa\".");
        assert_eq!(report.exit, Exit::Failure);
        assert!(
            report.trace.steps[0]
                .at
                .as_deref()
                .unwrap()
                .contains("report.rs:")
        );
    }
}
