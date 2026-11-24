//! Questions: asked on the terminal, answered by a variable or a flag
//! without one, and never asked where no one can answer.

use std::io::{BufRead, Read, Write};
use std::marker::PhantomData;
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use link_secretive::Secret;

use crate::render::{self, Ctx};
use crate::{Exit, Out, Phrase, Prose, Report, Sentence, Value, look};

/// What a question asks, and how its answer is checked.
#[derive(Clone, Debug)]
pub enum Asking {
    /// Words, and what Enter means, if anything.
    Text(Option<String>),
    /// Not echoed; its answer is a [`Secret`].
    Hidden,
    /// Yes or no, and what Enter means.
    Confirm(bool),
    /// One of these, by number, and what Enter means.
    Choose(Vec<(Value, Phrase)>, Option<usize>),
    /// This code typed back, to allow something once.
    Code(String),
}

/// A question whose answer is an `A`.
pub struct Question<A> {
    prompt: Sentence,
    why: Option<Prose>,
    asking: Asking,
    flag: Option<&'static str>,
    variable: Option<&'static str>,
    /// `--yes` given: a yes or no is answered yes without asking.
    yes: bool,
    unasked: Option<Sentence>,
    _answer: PhantomData<A>,
}

impl<A> Question<A> {
    fn new(prompt: Sentence, asking: Asking) -> Self {
        Question {
            prompt,
            why: None,
            asking,
            flag: None,
            variable: None,
            yes: false,
            unasked: None,
            _answer: PhantomData,
        }
    }

    /// What's worth knowing before answering, shown above it.
    pub fn why(mut self, why: impl Into<Prose>) -> Self {
        self.why = Some(why.into());
        self
    }

    /// The flag that answers it on the command line: `--model`.
    pub fn flag(mut self, flag: &'static str) -> Self {
        self.flag = Some(flag);
        self
    }

    /// The variable that answers it without a terminal.
    pub fn variable(mut self, variable: &'static str) -> Self {
        self.variable = Some(variable);
        self
    }

    /// Where no one can answer, not a failure: this sentence shows, as a
    /// note, and the question takes what Enter would (`Left off: lnk agent
    /// harness-permission add web allows it.`). Only a question with a default.
    pub fn unasked(mut self, says: Sentence) -> Self {
        self.unasked = Some(says);
        self
    }
}

impl Question<String> {
    pub fn text(prompt: Sentence) -> Self {
        Question::new(prompt, Asking::Text(None))
    }

    /// What Enter answers, shown in the prompt: `[Enter for defaults]`.
    pub fn default(mut self, answer: impl Into<String>) -> Self {
        self.asking = Asking::Text(Some(crate::clean(&answer.into())));
        self
    }
}

impl Question<Secret<String>> {
    pub fn hidden(prompt: Sentence) -> Self {
        Question::new(prompt, Asking::Hidden)
    }
}

impl Question<bool> {
    pub fn confirm(prompt: Sentence, default: bool) -> Self {
        Question::new(prompt, Asking::Confirm(default))
    }

    /// Whether `--yes` was given: it answers a yes or no yes, unasked,
    /// and is what a report says to add where no one can answer. A code
    /// is never answered by it.
    pub fn yes(mut self, given: bool) -> Self {
        if matches!(self.asking, Asking::Confirm(_)) {
            self.yes = given;
            self.flag = self.flag.or(Some("--yes"));
        }
        self
    }

    /// Allowed only if `code` is typed back: `Type 4F and Enter to allow it
    /// once`.
    pub fn code(prompt: Sentence, code: impl Into<String>) -> Self {
        Question::new(prompt, Asking::Code(code.into()))
    }
}

impl Question<usize> {
    pub fn choose(prompt: Sentence, options: Vec<(Value, Phrase)>) -> Self {
        Question::new(prompt, Asking::Choose(options, None))
    }

    pub fn default(mut self, i: usize) -> Self {
        if let Asking::Choose(_, d) = &mut self.asking {
            *d = Some(i);
        }
        self
    }
}

/// An answer, from what was typed.
pub trait Answer: Sized {
    /// `Err` says what a right answer looks like.
    fn take(raw: String, asking: &Asking) -> Result<Self, &'static str>;

    /// A hidden answer, read as a secret and handed on as one: only a
    /// [`Secret`] answer takes it.
    fn secret(secret: Secret<String>) -> Option<Self> {
        drop(secret);
        None
    }
}

impl Answer for String {
    fn take(raw: String, asking: &Asking) -> Result<Self, &'static str> {
        match (raw.trim(), asking) {
            ("", Asking::Text(Some(default))) => Ok(default.clone()),
            ("", _) => Err("an answer"),
            (t, _) => Ok(t.to_string()),
        }
    }
}

impl Answer for Secret<String> {
    /// Never from plain text: a hidden answer is read as a secret.
    fn take(_: String, _: &Asking) -> Result<Self, &'static str> {
        Err("an answer")
    }

    fn secret(secret: Secret<String>) -> Option<Self> {
        Some(secret)
    }
}

impl Answer for bool {
    fn take(raw: String, asking: &Asking) -> Result<Self, &'static str> {
        match asking {
            Asking::Code(code) => Ok(raw.trim().eq_ignore_ascii_case(code)),
            Asking::Confirm(default) => match raw.trim().to_ascii_lowercase().as_str() {
                "" => Ok(*default),
                "y" | "yes" => Ok(true),
                "n" | "no" => Ok(false),
                _ => Err("y or n"),
            },
            _ => Err("y or n"),
        }
    }
}

impl Answer for usize {
    fn take(raw: String, asking: &Asking) -> Result<Self, &'static str> {
        let Asking::Choose(options, default) = asking else {
            return Err("a number");
        };
        match (raw.trim(), default) {
            ("", Some(d)) => Ok(*d),
            (n, _) => match n.parse::<usize>() {
                Ok(n) if (1..=options.len()).contains(&n) => Ok(n - 1),
                _ => Err("one of the numbers"),
            },
        }
    }
}

pub(crate) fn ask<A: Answer>(out: &mut Out, q: Question<A>) -> Result<A, Report> {
    if let Some(variable) = q.variable {
        // A hidden answer's through `Secret::from_env`, which keeps what
        // the process guard took out of the environment at its start,
        // and keeps it from every child; any other's as it is.
        let given = match q.asking {
            Asking::Hidden => Secret::<String>::from_env(variable).and_then(A::secret),
            _ => std::env::var(variable)
                .ok()
                .and_then(|v| A::take(v, &q.asking).ok()),
        };
        if let Some(a) = given {
            return Ok(a);
        }
    }
    if q.yes {
        return A::take("y".into(), &q.asking).map_err(|_| unasked(out, &q));
    }
    if !out.can_ask() {
        return taken_unasked(out, &q);
    }
    let ctx = out.err_ctx().clone();
    out.say(&shown(&q, &ctx));
    let hidden = matches!(q.asking, Asking::Hidden);
    loop {
        if hidden {
            match read_secret().map_err(|e| Report::from(anyhow::Error::from(e)))? {
                None => {
                    out.say("\n");
                    return taken_unasked(out, &q);
                }
                Some(answer) => match answer.and_then(A::secret) {
                    Some(a) => return Ok(a),
                    None => out.say("Type an answer: "),
                },
            }
            continue;
        }
        let reply = read_line().map_err(|e| Report::from(anyhow::Error::from(e)))?;
        let Some(reply) = reply else {
            out.say("\n");
            return taken_unasked(out, &q);
        };
        match A::take(reply, &q.asking) {
            Ok(a) => return Ok(a),
            Err(right) => out.say(&format!("Type {right}: ")),
        }
    }
}

/// A question no one can answer here: its default, with its sentence,
/// if it has both; else a report.
fn taken_unasked<A: Answer>(out: &mut Out, q: &Question<A>) -> Result<A, Report> {
    match (&q.unasked, A::take(String::new(), &q.asking)) {
        (Some(says), Ok(a)) => {
            out.block(crate::Note(says.clone()));
            Ok(a)
        }
        _ => Err(unasked(out, q)),
    }
}

impl<A: Answer> Question<A> {
    /// Asked on `terminal`, from any thread, while a command runs: `None`
    /// when no answer comes within `wait`, which refuses it.
    pub fn ask_on_terminal(
        self,
        terminal: &mut crate::streams::Terminal,
        wait: Duration,
    ) -> std::io::Result<Option<A>> {
        self.ask_on(&mut terminal.0, wait)
    }

    /// [`Question::ask_on_terminal`], on `tty`: a test's stands in for it.
    pub(crate) fn ask_on(
        self,
        tty: &mut (impl Read + Write + AsRawFd),
        wait: Duration,
    ) -> std::io::Result<Option<A>> {
        let ctx = Ctx::terminal(&look::Settings::from_env(), look::WIDTH);
        tty.write_all(format!("\n{}", shown(&self, &ctx)).as_bytes())?;
        tty.flush()?;
        let until = Instant::now() + wait;
        loop {
            if !readable(tty, until.saturating_duration_since(Instant::now())) {
                tty.write_all(format!("\n{}No answer: refused.\n", look::INDENT).as_bytes())?;
                return Ok(None);
            }
            let mut buf = [0u8; 256];
            let n = tty.read(&mut buf)?;
            if n == 0 {
                return Ok(None);
            }
            let raw = String::from_utf8_lossy(&buf[..n]).trim_end().to_string();
            if let Asking::Code(code) = &self.asking {
                let said = match raw.trim().eq_ignore_ascii_case(code) {
                    true => "Allowed once.",
                    false => "Refused.",
                };
                tty.write_all(format!("{}{said}\n", look::INDENT).as_bytes())?;
            }
            match A::take(raw, &self.asking) {
                Ok(a) => return Ok(Some(a)),
                Err(right) => tty.write_all(format!("Type {right}: ").as_bytes())?,
            }
        }
    }
}

/// Whether `fd` has something to read within `wait`.
fn readable(fd: &impl AsRawFd, wait: Duration) -> bool {
    let mut poll = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = wait.as_millis().min(libc::c_int::MAX as u128) as libc::c_int;
    // SAFETY: one pollfd, for a descriptor its caller holds open.
    unsafe { libc::poll(&mut poll, 1, ms) > 0 }
}

/// The question as it's asked: why, the prompt in bold, the choices, and
/// what Enter does.
fn shown<A>(q: &Question<A>, ctx: &Ctx) -> String {
    let mut out = String::new();
    if let Some(why) = &q.why {
        out += &format!("{}\n", render::spans(why.spans(), ctx));
    }
    let prompt = render::spans(q.prompt.spans(), ctx);
    let bold = match ctx.bold {
        true => format!("{}{prompt}{:#}", look::BOLD, look::BOLD),
        false => prompt,
    };
    match &q.asking {
        Asking::Choose(options, default) => {
            out += &format!("{bold}\n");
            let wide = options
                .iter()
                .map(|(v, _)| render::value(v, ctx).chars().count())
                .max()
                .unwrap_or(0);
            for (i, (v, why)) in options.iter().enumerate() {
                let v = render::value(v, ctx);
                let pad = " ".repeat(wide - v.chars().count() + 3);
                out += &format!(
                    "{}{}  {v}{pad}{}\n",
                    look::INDENT,
                    i + 1,
                    render::spans(why.spans(), ctx)
                );
            }
            out += &match default {
                Some(d) => format!("Pick one, or Enter for {}: ", d + 1),
                None => "Pick one: ".to_string(),
            };
        }
        Asking::Confirm(true) => out += &format!("{bold} [Y/n]: "),
        Asking::Confirm(false) => out += &format!("{bold} [y/N]: "),
        Asking::Code(code) => {
            out += &format!(
                "{bold}\n{}Type {code} and Enter to allow it once; anything else refuses it: ",
                look::INDENT
            )
        }
        Asking::Text(Some(default)) => out += &format!("{bold} [Enter for {default}]: "),
        Asking::Text(None) | Asking::Hidden => out += &format!("{bold} "),
    }
    out
}

/// The report for a question no one can answer here: the command again,
/// answered by its flag or its variable.
fn unasked<A>(out: &Out, q: &Question<A>) -> Report {
    let mut r = Report::error(crate::sentence!(
        "Couldn't ask a question this needs answered."
    ))
    .why(Prose(q.prompt.spans().to_vec()))
    .exit(Exit::Usage);
    if let Some(flag) = q.flag {
        let argument = if flag == "--yes" && matches!(q.asking, Asking::Confirm(_)) {
            flag.to_owned()
        } else {
            format!("{flag} <answer>")
        };
        r = r.next(out.typed().and(&argument), "answer it with a flag");
    }
    if let Some(variable) = q.variable {
        r.why = Some(Prose(
            [
                q.prompt.spans().to_vec(),
                vec![crate::Span::Text(format!(
                    " {variable} answers it without a terminal."
                ))],
            ]
            .concat(),
        ));
    }
    r
}

/// A line from the terminal, without its newline; `None` at its end.
fn read_line() -> std::io::Result<Option<String>> {
    let mut line = String::new();
    Ok(match std::io::stdin().lock().read_line(&mut line)? {
        0 => None,
        _ => Some(line.trim_end_matches(['\n', '\r']).to_string()),
    })
}

/// A hidden answer from the terminal, not echoed, read straight into a
/// [`Secret`]: `None` at the input's end, `Some(None)` for an empty line.
fn read_secret() -> std::io::Result<Option<Option<Secret<String>>>> {
    let echo = |on: bool| {
        let _ = link_secretive::process::command("stty")
            .arg(if on { "echo" } else { "-echo" })
            .stdin(std::process::Stdio::inherit())
            .status();
    };
    echo(false);
    let mut stdin = std::io::stdin().lock();
    let read = match stdin.fill_buf() {
        Ok([]) => Ok(None),
        Ok(_) => Secret::read_line(&mut stdin).map(Some),
        Err(e) => Err(e),
    };
    echo(true);
    eprintln!();
    read
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    #[test]
    fn answers_are_checked() {
        let choose = Asking::Choose(
            vec![("a".into(), "first".into()), ("b".into(), "second".into())],
            Some(0),
        );
        assert_eq!(usize::take("".into(), &choose), Ok(0));
        assert_eq!(usize::take("2".into(), &choose), Ok(1));
        assert!(usize::take("3".into(), &choose).is_err());
        assert_eq!(bool::take("".into(), &Asking::Confirm(false)), Ok(false));
        assert_eq!(
            bool::take("4f".into(), &Asking::Code("4F".into())),
            Ok(true)
        );
        assert_eq!(
            bool::take("x".into(), &Asking::Code("4F".into())),
            Ok(false)
        );
    }

    #[test]
    fn no_terminal_is_a_report_saying_how_to_answer() {
        let (mut out, _) = Out::capture(false, &["lnk-agent", "start"]);
        let q = Question::choose(sentence!("Which model should it think with?"), vec![])
            .flag("--model.default");
        let r = out.ask(q).unwrap_err();
        assert_eq!(r.exit, Exit::Usage);
        assert_eq!(
            r.next.0[0].command.text(),
            "lnk agent start --model.default <answer>"
        );
    }

    #[test]
    fn yes_answers_unasked_and_is_what_a_report_names() {
        let (mut out, _) = Out::capture(false, &["lnk-sandbox", "run"]);
        let q = Question::confirm(sentence!("Run it?"), false).yes(true);
        assert_eq!(out.ask(q), Ok(true));
        let q = Question::confirm(sentence!("Run it?"), true).yes(false);
        let r = out.ask(q).unwrap_err();
        assert_eq!(r.next.0[0].command.text(), "lnk sandbox run --yes");
        let q = Question::code(sentence!("Allow it?"), "4F").yes(true);
        assert!(out.ask(q).unwrap_err().next.0.is_empty());
    }

    #[test]
    fn unasked_says_its_sentence_and_takes_the_default() {
        let (mut out, c) = Out::capture(false, &["lnk-agent", "start"]);
        let q = Question::confirm(sentence!("Allow web?"), false)
            .unasked(sentence!("Left off: web isn't allowed."));
        assert_eq!(out.ask(q), Ok(false));
        assert_eq!(c.stderr(), "  Left off: web isn't allowed.\n");
    }

    #[test]
    fn asked_on_a_terminal_from_a_thread_with_a_timeout() {
        use std::os::unix::net::UnixStream;
        let (mut tty, mut person) = UnixStream::pair().unwrap();
        person.write_all(b"4f\n").unwrap();
        let q = Question::code(sentence!("Allow it?"), "4F");
        let answer = std::thread::spawn(move || q.ask_on(&mut tty, Duration::from_secs(5)));
        assert_eq!(answer.join().unwrap().unwrap(), Some(true));
        let mut said = vec![0; 512];
        let n = person.read(&mut said).unwrap();
        assert!(String::from_utf8_lossy(&said[..n]).contains("Type 4F and Enter"));
        let (mut tty, _person) = UnixStream::pair().unwrap();
        let q = Question::code(sentence!("Allow it?"), "4F");
        assert_eq!(q.ask_on(&mut tty, Duration::ZERO).unwrap(), None);
    }

    const HIDDEN_ANSWER: &str = "LNK_TEST_FORMAT_HIDDEN_ANSWER";

    #[test]
    fn a_hidden_answers_variable_is_read_as_a_secret() {
        // The variable is the child's own: no test in this process shares it.
        let home = link_test_kit::TempDir::new("hidden-answer");
        let ran = link_test_kit::env::command(std::env::current_exe().unwrap(), &home)
            .args([
                "--exact",
                "ask::tests::hidden_answer_entry",
                "--ignored",
                "--nocapture",
            ])
            .env(HIDDEN_ANSWER, "s3cret")
            .output()
            .unwrap();
        assert!(
            ran.status.success(),
            "{}{}",
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr)
        );
    }

    #[test]
    #[ignore = "subprocess entry used by the hidden answer's variable test"]
    fn hidden_answer_entry() {
        let (mut out, _) = Out::capture(false, &["lnk-agent", "connect"]);
        let q = Question::hidden(sentence!("What's its key?")).variable(HIDDEN_ANSWER);
        assert_eq!(out.ask(q).unwrap().expose(), "s3cret");
        // A hidden answer is never made from plain text.
        assert!(Secret::<String>::take("s3cret".into(), &Asking::Hidden).is_err());
        // Read through `Secret::from_env`: taken, so what the guard took at
        // a module's start answers too, and no child is given it.
        let given = link_secretive::process::environment().any(|(n, _)| n == HIDDEN_ANSWER);
        assert!(!given);
    }

    #[test]
    fn enter_answers_a_text_question_with_its_default() {
        let q = Question::text(sentence!("Which modules?")).default("defaults");
        assert_eq!(
            shown(&q, &Ctx::plain()),
            "Which modules? [Enter for defaults]: "
        );
        assert_eq!(String::take("".into(), &q.asking), Ok("defaults".into()));
        assert_eq!(
            String::take(" agent ".into(), &q.asking),
            Ok("agent".into())
        );
        assert!(String::take("".into(), &Asking::Text(None)).is_err());
        // Without a terminal, a question with a default takes it, said.
        let (mut out, _) = Out::capture(false, &["lnk", "up"]);
        let q = q.unasked(sentence!("Installing the defaults."));
        assert_eq!(out.ask(q).unwrap(), "defaults");
    }

    #[test]
    fn a_choice_shows_its_options() {
        let q = Question::choose(
            sentence!("Which model?"),
            vec![
                ("qwen3:8b".into(), phrase!("here, free")),
                ("claude".into(), phrase!("hosted")),
            ],
        )
        .default(0);
        assert_eq!(
            shown(&q, &Ctx::plain()),
            "Which model?\n  1  qwen3:8b   here, free\n  2  claude     hosted\nPick one, or Enter for 1: "
        );
    }
}
