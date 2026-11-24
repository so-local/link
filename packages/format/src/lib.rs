//! Everything `lnk` prints, as typed blocks one formatter renders: styled
//! to a terminal, plain to a pipe or a file, and as `--json`.
//!
//! A command writes blocks to an [`Out`] as they come and returns how it
//! ended, [`Done`] or a [`Report`]; [`run`] does the rest. Text in a block
//! is never a bare `String`: it's a [`Sentence`], a [`Phrase`] or
//! [`Prose`], each checked where it's made, holding typed [`Value`]s. How
//! any of it looks is [`look`]'s, one file.
//!
//! ```text
//! fn main() -> std::process::ExitCode {
//!     link_format::run(|out| {
//!         let agent = Name::new("agent", "my-otter");
//!         out.block(Fields::new().row("Name", &agent).key("Model", "qwen3:8b"));
//!         Ok(Done::success(sentence!("Started {}.", &agent))
//!             .next(cmd!("lnk agent show"), "see what it's doing"))
//!     })
//! }
//! ```

pub mod answer;
mod ask;
mod block;
mod command;
pub mod daemon;
pub mod inner;
pub mod issue;
pub mod log;
pub mod look;
pub mod nexts;
mod out;
pub mod record;
pub mod render;
mod report;
pub mod scrub;
mod state;
mod stop;
pub mod streams;
pub mod time;
mod trace;
mod value;
mod words;

pub use block::*;
pub use command::{Command, Word};
pub use out::{
    Activity, Answer, Bar, Captured, Done, HandOver, INNER, Live, Notes, Out, Question, Steps,
    select_json, set_invocation,
};
pub use record::Record;
pub use report::{
    Exit, Kind, MODULE_CODES, NotInstalled, Refused, Refuser, Report, Usage, refused,
};
pub use state::{State, Tone, words_are_lowercase};
pub use trace::{Step, Trace, Within, WithinExt};
pub use value::*;
pub use words::{Phrase, Prose, Sentence, Span};

use std::cell::RefCell;
use std::process::ExitCode;

/// Runs a command: its blocks rendered, its report if it failed, a panic
/// made a report too, and its exit code.
pub fn run(command: impl FnOnce(&mut Out) -> Result<Done, Report>) -> ExitCode {
    run_with(Machine::default(), command)
}

/// What this machine gives a run, which `link_settings::output::run`
/// reads from its folder: `[output]` from its settings file, and the
/// folder a failure's record is written to.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct Machine {
    pub output: look::Settings,
    pub records: Option<std::path::PathBuf>,
    /// Prepare a record destination only when a bug needs a record.
    pub prepare_records: Option<fn(&std::path::Path) -> anyhow::Result<()>>,
}

impl Machine {
    pub fn new(output: look::Settings, records: Option<std::path::PathBuf>) -> Self {
        Machine {
            output,
            records,
            prepare_records: None,
        }
    }
}

/// [`run`], on this machine as its folder has it.
pub fn run_with(
    machine: Machine,
    command: impl FnOnce(&mut Out) -> Result<Done, Report>,
) -> ExitCode {
    let mut out = Out::with(machine);
    let result = catch(|| command(&mut out));
    ExitCode::from(out.finish(result))
}

thread_local! {
    /// A panic's message and place, kept by the hook [`catch`] sets, on
    /// the thread that panicked.
    static PANIC: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

fn catch(f: impl FnOnce() -> Result<Done, Report>) -> Result<Done, Report> {
    std::panic::set_hook(Box::new(|info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let at = info.location().map(trace::place).unwrap_or_default();
        PANIC.with(|p| *p.borrow_mut() = Some((message, at)));
    }));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    let _ = std::panic::take_hook();
    result.unwrap_or_else(|_| {
        let (message, at) = PANIC.with(|p| p.borrow_mut().take()).unwrap_or_default();
        let mut r = Report::bug(sentence!("Something inside Link went wrong."))
            .why(prose!("It's a bug in Link, not in what you did."));
        r.trace.steps = vec![Step {
            what: clean(&message),
            at: Some(at),
            program: None,
        }];
        Err(r)
    })
}

/// Whether anything was written [`aside`], for the blank line before a
/// command's result.
pub(crate) static ASIDE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A block from code that holds no [`Out`], such as a library's warning
/// or a step it takes: shown at once on stderr, as an `Out` shows what's
/// about the run, styled on a terminal as the variables set, plain
/// elsewhere. The one place such code writes.
pub fn aside(block: impl Into<Block>) {
    use std::io::IsTerminal;
    let ctx = match std::io::stderr().is_terminal() {
        true => render::Ctx::terminal(&look::Settings::from_env(), out::terminal_width()),
        false => render::Ctx::plain(),
    };
    aside_to(&mut std::io::stderr(), &block.into(), &ctx);
}

fn aside_to(to: &mut impl std::io::Write, block: &Block, ctx: &render::Ctx) {
    let _ = to.write_all(render::block(block, ctx).as_bytes());
    ASIDE.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// A question from code that holds no [`Out`], as [`aside`] shows a
/// block: asked as [`Out::ask`] asks, on this terminal, and never where
/// there's none to answer it.
pub fn ask_aside<A: Answer>(q: Question<A>) -> Result<A, Report> {
    Out::from_env().ask(q)
}

/// `text` safe to show: without control characters, or characters that
/// show nothing themselves but change how text around them shows (a
/// zero-width one, or one that reverses its direction).
pub fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() && !invisible(*c))
        .collect()
}

pub(crate) fn invisible(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{034F}' | '\u{061C}' | '\u{115F}' | '\u{1160}' | '\u{17B4}' | '\u{17B5}'
        | '\u{180B}'..='\u{180F}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{206F}' | '\u{3164}' | '\u{FE00}'..='\u{FE0F}' | '\u{FEFF}'
        | '\u{FFA0}' | '\u{FFF0}'..='\u{FFFB}' | '\u{E0000}'..='\u{E0FFF}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_is_a_report_with_its_place() {
        let r = catch(|| panic!("index out of bounds")).unwrap_err();
        assert_eq!(r.headline.to_string(), "Something inside Link went wrong.");
        assert_eq!(r.trace.steps[0].what, "index out of bounds");
        assert!(r.trace.steps[0].at.as_deref().unwrap().contains("lib.rs:"));
    }

    #[test]
    fn an_aside_is_a_block_on_its_own() {
        let mut said = Vec::new();
        let warning = Report::warning(sentence!("The Keychain refused it."));
        aside_to(&mut said, &warning.into(), &render::Ctx::plain());
        assert_eq!(
            String::from_utf8(said).unwrap(),
            "! The Keychain refused it.\n"
        );
        assert!(ASIDE.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn clean_drops_what_could_hide_text() {
        assert_eq!(clean("a\x1b]52;c;x\x07b\n"), "a]52;c;xb");
        assert_eq!(clean("ok\u{202E}gpj.exe\u{200B}"), "okgpj.exe");
    }
}

#[cfg(test)]
link_test_kit::program!(system);
