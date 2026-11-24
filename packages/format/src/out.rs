//! `Out`: where a command writes its blocks, as they come. It alone knows
//! the terminal, the streams, the rendering and the settings: the result
//! goes to stdout, held to the end so the success line leads it; what's
//! about the run (progress, reports, Next, questions) goes to stderr at
//! once; `--json` collects every block into one document.

use std::io::Write;
use std::sync::{Arc, Mutex};

use crate::block::*;
use crate::look;
use crate::render::{self, Ctx, Footer};
use crate::{Command, Phrase, Report, Sentence, Span, Value};

/// How a command ended well: what it did, and what's next.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Done {
    /// None for a list or a show: its result is its block.
    pub success: Option<Success>,
    pub next: Next,
    /// Another program's exit code, passed through as this command's:
    /// not on the exit codes' list, and not a failure of this one.
    pub passed: Option<u8>,
}

impl Done {
    pub fn success(done: Sentence) -> Self {
        Done {
            success: Some(Success::new(done)),
            next: Next::default(),
            passed: None,
        }
    }

    /// A command that did all it can and waits on the person for the
    /// rest, which a block above says: no failure, nor done.
    pub fn waiting(done: Sentence) -> Self {
        let mut success = Success::new(done);
        success.waiting = true;
        Done {
            success: Some(success),
            next: Next::default(),
            passed: None,
        }
    }

    /// A command that ran another program as its own (`lnk sandbox run`)
    /// and exits with that program's code, as a shell would.
    pub fn passed(code: i32) -> Self {
        Done {
            passed: Some(code as u8),
            ..Done::default()
        }
    }

    /// A command whose result is its blocks: a list, a status.
    pub fn shown() -> Self {
        Done::default()
    }

    #[track_caller]
    pub fn next(mut self, command: Command, why: impl Into<Phrase>) -> Self {
        self.next.push(command, why);
        self
    }

    pub fn changed(mut self, what: impl Into<Phrase>, kept: impl Into<Value>) -> Self {
        if let Some(s) = self.success.take() {
            self.success = Some(s.changed(what, kept));
        }
        self
    }
}

type Sink = Arc<Mutex<dyn Write + Send>>;

pub struct Out {
    json: bool,
    out: Sink,
    err: Sink,
    out_ctx: Ctx,
    err_ctx: Ctx,
    out_tty: bool,
    err_tty: bool,
    stdin_tty: bool,
    held: Vec<Block>,
    record: Vec<Block>,
    said: Arc<std::sync::atomic::AtomicBool>,
    typed: Command,
    path: String,
    reply: Option<serde_json::Value>,
    lines: bool,
    records: Option<std::path::PathBuf>,
    prepare_records: Option<fn(&std::path::Path) -> anyhow::Result<()>>,
    inner: bool,
    /// Writes to this process's stderr, where [`crate::aside`] writes too.
    process: bool,
}

/// Set on a command another runs for its own (`link_core::lnk()`): an
/// inner run shows no Next and no report's footer, which are the outer
/// command's. Its value is the folder an inner run that fails leaves its
/// trace in, for the outer report ([`crate::inner`]).
pub const INNER: &str = "LNK_INNER";

impl Out {
    /// For this process: its arguments, its streams and the variables.
    pub fn from_env() -> Self {
        Out::with(crate::Machine::default())
    }

    /// For this process on `machine`: its arguments, its streams, its
    /// `[output]` under the variables, and where a failure's record goes.
    pub fn with(machine: crate::Machine) -> Self {
        use std::io::IsTerminal;
        let settings = machine.output.under_env();
        let width = terminal_width();
        let ctx = |tty: bool| match tty {
            true => Ctx::terminal(&settings, width),
            false => Ctx::plain(),
        };
        let (typed, path) = INVOCATION.get().cloned().unwrap_or_else(|| typed(&[]));
        if let Some(records) = &machine.records {
            crate::inner::clear(&crate::inner::folder(records));
        }
        Out {
            json: false,
            out: Arc::new(Mutex::new(std::io::stdout())),
            err: Arc::new(Mutex::new(std::io::stderr())),
            out_ctx: ctx(std::io::stdout().is_terminal()),
            err_ctx: ctx(std::io::stderr().is_terminal()),
            out_tty: std::io::stdout().is_terminal(),
            err_tty: std::io::stderr().is_terminal(),
            stdin_tty: std::io::stdin().is_terminal(),
            held: Vec::new(),
            record: Vec::new(),
            said: Arc::default(),
            typed,
            path,
            reply: None,
            lines: false,
            records: machine.records,
            prepare_records: machine.prepare_records,
            inner: std::env::var_os(INNER).is_some_and(|v| !v.is_empty()),
            process: true,
        }
    }

    /// One that writes to buffers, for tests: plain, or `--json`.
    pub fn capture(json: bool, args: &[&str]) -> (Self, Captured) {
        let out = Arc::new(Mutex::new(Vec::new()));
        let err = Arc::new(Mutex::new(Vec::new()));
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (typed, path) = typed(&args);
        let o = Out {
            json,
            out: out.clone(),
            err: err.clone(),
            out_ctx: Ctx::plain(),
            err_ctx: Ctx::plain(),
            out_tty: false,
            err_tty: false,
            stdin_tty: false,
            held: Vec::new(),
            record: Vec::new(),
            said: Arc::default(),
            typed,
            path,
            reply: None,
            lines: false,
            records: None,
            prepare_records: None,
            inner: false,
            process: false,
        };
        (o, Captured { out, err })
    }

    /// The command itself, without its arguments, as the context line
    /// and a retry show it: what a module sets when it knows better than
    /// its first word.
    pub fn at(&mut self, path: &[&str]) {
        self.path = path.join(" ");
    }

    /// The flags this command declares secret (`--token`): what follows
    /// each shows as `<secret>` in its record and in a retry.
    pub fn secret(&mut self, flags: &[&str]) {
        self.typed = self.typed.hiding(flags);
    }

    /// The command as the person typed it: what a retry suggests.
    pub fn typed(&self) -> Command {
        self.typed.clone()
    }

    pub fn is_json(&self) -> bool {
        self.json || SELECTED.get() == Some(&true)
    }

    fn write_err(&mut self, text: &str) {
        let _ = self.err.lock().map(|mut e| e.write_all(text.as_bytes()));
        self.said.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn write_out(&mut self, text: &str) {
        let _ = self.out.lock().map(|mut o| o.write_all(text.as_bytes()));
    }

    /// What `--json` prints on success, in place of the blocks: the typed
    /// reply another program reads (`lnk box list --json`'s boxes), its
    /// shape the command's own. A person's run shows the blocks. A
    /// failure prints nothing on stdout.
    pub fn reply(&mut self, reply: &impl serde::Serialize) {
        if self.is_json() {
            self.reply = serde_json::to_value(reply).ok();
        }
    }

    /// An answer another program gave, such as an agent's action's: with
    /// `--json` the reply, as it is; for a person, its members as blocks
    /// ([`crate::answer::blocks`]), never the JSON.
    pub fn answer(&mut self, answer: &serde_json::Value) {
        match self.is_json() {
            true => self.reply(answer),
            false => {
                for block in crate::answer::blocks(answer) {
                    self.block(block);
                }
            }
        }
    }

    /// One item of a reply read a line at a time (`lnk sandbox log show
    /// --json`): with `--json`, one JSON object on its own line, written
    /// at once, and nothing added after the last. A person's run shows
    /// the blocks.
    pub fn reply_line(&mut self, item: &impl serde::Serialize) {
        if !self.is_json() {
            return;
        }
        if let Ok(line) = serde_json::to_string(item) {
            self.write_out(&format!("{}\n", escaped(&line)));
            let _ = self.out.lock().map(|mut o| o.flush());
        }
        self.lines = true;
    }

    /// What `--json` prints on success, when it holds a credential (an
    /// access token): never to a terminal, where scrollback or a shared
    /// screen would keep it, so a terminal is refused.
    pub fn reply_private(&mut self, reply: &impl serde::Serialize) -> Result<(), Report> {
        if self.out_tty {
            return Err(Report::refusal(
                crate::Refuser::Policy("credentials".into()),
                crate::sentence!(
                    "This prints a credential, so only to another program, never to a terminal."
                ),
            ));
        }
        self.json = true;
        self.reply(reply);
        Ok(())
    }

    /// A block a person needs while the command runs, such as a code to
    /// type elsewhere: shown at once on stderr, not held for the end.
    pub fn now(&mut self, block: impl Into<Block>) {
        let block = block.into();
        if self.is_json() {
            // A person still needs it while stdout is JSON: plain on
            // stderr, and recorded.
            let text = render::block(&block, &Ctx::plain());
            self.write_err(&format!("\n{text}\n"));
            self.record.push(block);
            return;
        }
        let text = render::block(&block, &self.err_ctx);
        self.write_err(&format!("\n{text}\n"));
    }

    /// A block: the result is held for the end, a report (a warning)
    /// shows at once.
    pub fn block(&mut self, block: impl Into<Block>) {
        let block = block.into();
        if self.is_json() {
            // A warning or a note still reaches a person on stderr, where
            // a program running this with --json passes it on.
            if matches!(block, Block::Report(_) | Block::Note(_)) {
                let text = render::block(&block, &Ctx::plain());
                self.write_err(&text);
            }
            self.record.push(block);
        } else if block.is_result() {
            self.held.push(block);
        } else {
            let text = render::block(&block, &self.err_ctx);
            self.write_err(&text);
        }
    }

    /// A handle another thread writes notes with while the command holds
    /// this `Out`: each shows at once on stderr, as [`Out::block`] shows
    /// a [`Note`].
    pub fn notes(&self) -> Notes {
        Notes {
            err: self.err.clone(),
            ctx: match self.is_json() {
                true => Ctx::plain(),
                false => self.err_ctx.clone(),
            },
            said: self.said.clone(),
        }
    }

    /// The next step of a counted flow: `[2/3] Starting my-otter...`.
    pub fn step(&mut self, steps: &mut Steps, what: Phrase) {
        steps.at += 1;
        self.stoppable(steps, &what);
        self.block(Progress {
            step: Some(steps.at),
            of: Some(steps.total),
            what,
        });
    }

    /// Something that takes a while with no count: a spinner and its
    /// clock on a terminal, until the `Activity` is dropped; a line once
    /// elsewhere.
    pub fn activity(&mut self, what: Phrase) -> Activity {
        self.activity_at(None, what)
    }

    /// A step of a counted flow that waits: `[2/4] Waiting for qa-1 ⠹ 0:42`.
    pub fn wait_step(&mut self, steps: &mut Steps, what: Phrase) -> Activity {
        steps.at += 1;
        self.stoppable(steps, &what);
        self.activity_at(Some((steps.at, steps.total)), what)
    }

    /// What Ctrl-C says while `steps` is at its step: where it stopped and
    /// a Next to start again, written whole.
    fn stopped(&self, steps: &Steps, what: &Phrase) -> String {
        let mut spans = vec![Span::Text(format!(
            "Stopped at step {} of {}: ",
            steps.at, steps.total
        ))];
        spans.extend(what.0.iter().cloned());
        spans.push(Span::Text(".".into()));
        let mut next = Next::default();
        next.push(self.typed.clone(), crate::phrase!("start again"));
        let lead = if self.err_tty { "\r\x1b[2K" } else { "" };
        format!(
            "{lead}{}\n{}",
            render::block(&Note(Sentence(spans)).into(), &self.err_ctx).trim_end(),
            render::block(&next.into(), &self.err_ctx)
        )
    }

    /// Ctrl-C in this step of a flow stops the command, saying where.
    fn stoppable(&self, steps: &Steps, what: &Phrase) {
        if self.process && !self.inner {
            crate::stop::arm(self.stopped(steps, what));
        }
    }

    fn activity_at(&mut self, step: Option<(usize, usize)>, what: Phrase) -> Activity {
        let p = Progress {
            step: step.map(|s| s.0),
            of: step.map(|s| s.1),
            what,
        };
        if self.is_json() {
            self.block(p);
            return Activity::none();
        }
        let line = render::block(&p.clone().into(), &self.err_ctx);
        if !self.err_tty {
            self.block(p);
            return Activity::still(
                line.trim_end().trim_end_matches(look::ELLIPSIS).to_string(),
                self.err.clone(),
                look::STILL_EVERY,
            );
        }
        self.said.store(true, std::sync::atomic::Ordering::Relaxed);
        Activity::spin(
            line.trim_end().trim_end_matches(look::ELLIPSIS).to_string(),
            self.err.clone(),
        )
    }

    /// A bar toward a known total of bytes.
    pub fn bar(&mut self, what: Phrase, total: u64) -> Bar {
        let line = crate::words::plain(&what.0);
        self.block(Progress {
            step: None,
            of: None,
            what,
        });
        Bar {
            what: line,
            total,
            done: 0,
            tty: self.err_tty && !self.is_json(),
            err: self.err.clone(),
        }
    }

    /// Bytes for another program, as they are: never styled, wrapped or
    /// cleaned. In `--json`, a string block.
    pub fn data(&mut self, bytes: &[u8]) {
        match self.is_json() {
            true => self
                .record
                .push(Block::Text(Text(crate::Prose(vec![crate::Span::Text(
                    String::from_utf8_lossy(bytes).into_owned(),
                )])))),
            false => {
                let _ = self.out.lock().map(|mut o| o.write_all(bytes));
            }
        }
    }

    /// Output that runs as long as the person keeps it open: `logs
    /// --live`, a chat. Each line shows at once; `--json` writes one JSON
    /// object a line.
    pub fn live(&mut self) -> Live<'_> {
        Live { out: self }
    }

    /// The terminal lent to another program (`ssh`, a harness's own
    /// screen): anything waiting is written first, and nothing of Link's
    /// draws until the program is done.
    pub fn hand_over(&mut self) -> HandOver {
        // Ctrl-C is the other program's now, not this flow's.
        crate::stop::disarm();
        let _ = self.err.lock().map(|mut e| e.flush());
        let _ = self.out.lock().map(|mut o| o.flush());
        HandOver(())
    }

    /// Asks a question on the terminal; its variable answers it without
    /// one. With no terminal, or with `--json`, it's never asked: a
    /// report says how to answer it on the command line.
    pub fn ask<A: Answer>(&mut self, q: Question<A>) -> Result<A, Report> {
        crate::ask::ask(self, q)
    }

    /// Whether a question can be asked here: a terminal on stdin and
    /// stderr, and not `--json`. A command that asks only when it can
    /// branches on it; [`Out::ask`] needs no check of its own.
    pub fn can_ask(&self) -> bool {
        self.stdin_tty && self.err_tty && !self.is_json()
    }

    pub(crate) fn say(&mut self, text: &str) {
        self.write_err(text);
    }

    pub(crate) fn err_ctx(&self) -> &Ctx {
        &self.err_ctx
    }

    /// How the command ended, written, and its exit code.
    pub fn finish(mut self, result: Result<Done, Report>) -> u8 {
        crate::stop::disarm();
        match &result {
            Ok(done) => crate::nexts::check(&done.next, false, &self.typed),
            Err(r) => crate::nexts::check(&r.next, true, &self.typed),
        }
        let result = self.carry(result);
        if self.is_json() {
            return self.finish_json(result);
        }
        // On a terminal, a blank line between Link's progress and the result.
        let said_before = self.said.load(std::sync::atomic::Ordering::Relaxed)
            || (self.process && crate::ASIDE.load(std::sync::atomic::Ordering::Relaxed));
        let blank_first = said_before && self.out_tty;
        let mut results = Vec::new();
        let (exit, next, report) = match result {
            Ok(done) => {
                results.extend(done.success.map(Block::Success));
                (done.passed.unwrap_or(0), done.next, None)
            }
            Err(r) => {
                let code = match r.exit.code() {
                    0 => 1,
                    c => c,
                };
                (code, Next::default(), Some(r))
            }
        };
        results.append(&mut self.held);
        let mut text = String::new();
        for (i, b) in results.iter().enumerate() {
            if i > 0 || blank_first {
                text.push('\n');
            }
            text += &render::block(b, &self.out_ctx);
        }
        self.write_out(&text);
        let said = said_before || !results.is_empty();
        if let (Some(r), true) = (&report, self.inner) {
            let text = render::report(r, None, &self.err_ctx);
            self.write_err(&format!("{}{text}", if said { "\n" } else { "" }));
        } else if self.inner {
            // The outer command says what's next.
        } else if let Some(r) = report {
            let footer = Footer {
                version: crate::record::version(),
                platform: crate::record::platform(),
                command: self.path.clone(),
                record: self.record(&r),
            };
            let text = render::report(&r, Some(&footer), &self.err_ctx);
            self.write_err(&format!("{}{text}", if said { "\n" } else { "" }));
        } else if !next.is_empty() {
            let text = render::block(&next.into(), &self.err_ctx);
            self.write_err(&format!("{}{text}", if said { "\n" } else { "" }));
        }
        exit
    }

    /// The report written to a record in this machine's records, and its
    /// path: none where there's no folder or it can't be written, which
    /// never fails the command.
    fn record(&self, r: &Report) -> Option<std::path::PathBuf> {
        let dir = self.records.as_deref().filter(|_| r.is_bug())?;
        if let Some(prepare) = self.prepare_records {
            prepare(dir).ok()?;
        }
        crate::Record::new(r, self.typed.clone()).write(dir).ok()
    }

    /// The trace across module calls: an inner run's that failed under
    /// this report's, and this report's left for the run that ran this
    /// one, when this is an inner run.
    fn carry(&self, result: Result<Done, Report>) -> Result<Done, Report> {
        let left = self.records.as_deref().map(crate::inner::folder);
        let result = match (result, &left) {
            (Err(r), Some(dir)) => Err(match crate::inner::take(dir) {
                Some(inner) => r.under(inner),
                None => r,
            }),
            (result, dir) => {
                dir.as_deref().map(crate::inner::clear);
                result
            }
        };
        if let (Err(r), true) = (&result, self.inner) {
            crate::inner::leave(&self.path, &r.trace);
        }
        result
    }

    fn finish_json(mut self, result: Result<Done, Report>) -> u8 {
        let passed = match &result {
            Ok(done) => done.passed.unwrap_or(0),
            Err(_) => 0,
        };
        if let (Ok(_), Some(reply)) = (&result, self.reply.take()) {
            self.write_out(&format!("{}\n", escaped(&reply.to_string())));
            return passed;
        }
        // A command that doesn't print blocks prints its own JSON, and one
        // replying a line at a time has: nothing is added to it.
        if let Ok(done) = &result
            && (self.lines || self.record.is_empty())
            && done.success.is_none()
            && done.next.is_empty()
        {
            return passed;
        }
        let exit = match result {
            Ok(done) => {
                self.record.extend(done.success.map(Block::Success));
                if !done.next.is_empty() {
                    self.record.push(done.next.into());
                }
                passed
            }
            Err(r) => {
                // Stdout holds only a success's answer, so a program
                // reading it never takes a failure for one: the report is
                // on stderr, where a program running this reads why.
                if !self.inner {
                    self.record(&r);
                }
                let text = render::report(&r, None, &Ctx::plain());
                self.write_err(&text);
                return r.exit.code().max(1);
            }
        };
        let doc = serde_json::json!({ "blocks": self.record, "exit": exit });
        let text = format!("{doc}\n");
        self.write_out(&text);
        exit
    }
}

/// Notes from another thread: see [`Out::notes`].
#[derive(Clone)]
pub struct Notes {
    err: Sink,
    ctx: Ctx,
    said: Arc<std::sync::atomic::AtomicBool>,
}

impl Notes {
    pub fn note(&self, says: Sentence) {
        let text = render::block(&Note(says).into(), &self.ctx);
        let _ = self.err.lock().map(|mut e| e.write_all(text.as_bytes()));
        self.said.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// What a captured `Out` wrote.
pub struct Captured {
    out: Arc<Mutex<Vec<u8>>>,
    err: Arc<Mutex<Vec<u8>>>,
}

impl Captured {
    pub fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.out.lock().unwrap()).into_owned()
    }

    pub fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.err.lock().unwrap()).into_owned()
    }
}

/// Whether this process answers in JSON, as its parser chose from the
/// command it parsed ([`select_json`]).
static SELECTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

static INVOCATION: std::sync::OnceLock<(Command, String)> = std::sync::OnceLock::new();

/// Record the invocation supplied by the shared parser for diagnostics.
pub fn set_invocation(arguments: &[std::ffi::OsString]) {
    let arguments: Vec<String> = arguments
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let _ = INVOCATION.set(typed(&arguments));
}

/// Choose this process's output from the command its parser read: JSON
/// where the command asked for it (`--json`), the blocks otherwise. The
/// first choice stands. Only a parser chooses, never a scan of the
/// arguments.
pub fn select_json(json: bool) {
    let _ = SELECTED.set(json);
}

/// JSON as a reply prints it: C1 controls and characters that hide or
/// turn text around them written as `\uXXXX`, so a terminal shown the
/// line draws what's in it.
fn escaped(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        match ('\u{80}'..='\u{9f}').contains(&c) || crate::invisible(c) {
            true => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    out += &format!("\\u{u:04x}");
                }
            }
            false => out.push(c),
        }
    }
    out
}

/// The command as typed, and its path: `lnk-agent start --x` is `lnk
/// agent start`, its arguments left out.
fn typed(args: &[String]) -> (Command, String) {
    let program = args
        .first()
        .and_then(|a| std::path::Path::new(a).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "lnk".into());
    let mut words: Vec<String> = match program.strip_prefix("lnk-") {
        Some(module) => vec!["lnk".into(), module.into()],
        None => vec!["lnk".into()],
    };
    // `lnk` runs `lnk-box box create ...`: the group's word, once.
    let skip = match (program.strip_prefix("lnk-"), args.get(1)) {
        (Some(module), Some(first)) if first == module => 2,
        _ => 1,
    };
    let mut path = words.join(" ");
    if let Some(first) = args.iter().skip(skip).find(|a| !a.starts_with('-')) {
        path = format!("{path} {}", crate::clean(first));
    }
    words.extend(args.iter().skip(skip).cloned());
    (Command::typed(words), path)
}

pub(crate) fn terminal_width() -> usize {
    if let Some(n) = std::env::var("COLUMNS").ok().and_then(|c| c.parse().ok()) {
        return n;
    }
    // SAFETY: TIOCGWINSZ only fills the struct it's given.
    unsafe {
        let mut size: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDERR_FILENO, libc::TIOCGWINSZ, &mut size) == 0 && size.ws_col > 0 {
            return size.ws_col as usize;
        }
    }
    look::WIDTH
}

/// A counted flow's steps: `Steps::new(3)`, then [`Out::step`].
#[derive(Clone, Copy, Debug)]
pub struct Steps {
    at: usize,
    total: usize,
}

impl Steps {
    pub fn new(total: usize) -> Self {
        Steps { at: 0, total }
    }
}

/// A spinner until dropped; then its line stays, with how long it took.
/// With no terminal, a line every [`look::STILL_EVERY`] saying it's still
/// going, until dropped.
pub struct Activity {
    stop: Option<(Stop, std::thread::JoinHandle<()>)>,
}

/// How an activity's thread is told to end.
enum Stop {
    /// A spinner's, read between its frames.
    Spinner(Arc<std::sync::atomic::AtomicBool>),
    /// A quiet wait's, which ends it at once when dropped.
    Still(std::sync::mpsc::Sender<()>),
}

impl Activity {
    fn none() -> Self {
        Activity { stop: None }
    }

    /// With no terminal: `line`, already written, said again every
    /// `every` with how long it's been, until dropped.
    fn still(line: String, err: Sink, every: std::time::Duration) -> Self {
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        let handle = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(every)
            {
                let took = crate::time::elapsed(start.elapsed());
                let _ = err.lock().map(|mut e| {
                    writeln!(e, "{line}, still going after {took}").and_then(|_| e.flush())
                });
            }
        });
        Activity {
            stop: Some((Stop::Still(stop), handle)),
        }
    }

    fn spin(line: String, err: Sink) -> Self {
        use std::sync::atomic::{AtomicBool, Ordering};
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let handle = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            let mut i = 0;
            while !stopping.load(Ordering::Relaxed) {
                let frame = look::SPINNER[i % look::SPINNER.len()];
                let clock = crate::time::clock(start.elapsed());
                let shown = activity_frame(&line, frame, &clock, terminal_width());
                let _ = err
                    .lock()
                    .map(|mut e| write!(e, "\r\x1b[2K{shown}").and_then(|_| e.flush()));
                i += 1;
                #[expect(
                    clippy::disallowed_methods,
                    reason = "a spinner turns on a clock, waiting on nothing"
                )]
                std::thread::sleep(look::SPIN_EVERY);
            }
            let took = crate::time::elapsed(start.elapsed());
            let _ = err
                .lock()
                .map(|mut e| writeln!(e, "\r\x1b[2K{line}{} {took}", look::ELLIPSIS));
        });
        Activity {
            stop: Some((Stop::Spinner(stop), handle)),
        }
    }
}

/// Keep a redraw on one row, leaving the last column free so the terminal
/// cannot wrap. The completed activity still prints its description whole.
fn activity_frame(line: &str, frame: &str, clock: &str, columns: usize) -> String {
    let room = columns.saturating_sub(1);
    let tail = format!("  {frame} {clock}");
    if render::width(line) + render::width(&tail) <= room {
        return format!("{line}{tail}");
    }
    let plain = anstream::adapter::strip_str(line).to_string();
    let prefix = room.saturating_sub(render::width(&tail) + look::ELLIPSIS.len());
    format!(
        "{}{}{tail}",
        plain.chars().take(prefix).collect::<String>(),
        look::ELLIPSIS
    )
    .chars()
    .take(room)
    .collect()
}

impl Drop for Activity {
    fn drop(&mut self) {
        if let Some((stop, handle)) = self.stop.take() {
            match stop {
                Stop::Spinner(stop) => stop.store(true, std::sync::atomic::Ordering::Relaxed),
                Stop::Still(stop) => drop(stop),
            }
            let _ = handle.join();
        }
    }
}

/// A bar toward a total: drawn in place on a terminal, its total written
/// when it's dropped.
pub struct Bar {
    what: String,
    total: u64,
    done: u64,
    tty: bool,
    err: Sink,
}

impl Bar {
    pub fn add(&mut self, n: u64) {
        self.set(self.done + n, self.total);
    }

    /// How far it is toward a total that may have grown: a download
    /// whose parts are found as it goes.
    pub fn set(&mut self, done: u64, total: u64) {
        self.total = total;
        self.done = done.min(total);
        self.draw();
    }

    /// What it's doing now, when that changes as it goes (`verifying`):
    /// another program's words, cleaned.
    pub fn status(&mut self, what: &str) {
        let what = crate::clean(what);
        if what != self.what {
            if !self.tty {
                let line = format!("{what}...\n");
                let _ = self.err.lock().map(|mut e| e.write_all(line.as_bytes()));
            }
            self.what = what;
            self.draw();
        }
    }

    fn draw(&mut self) {
        if !self.tty || self.total == 0 {
            return;
        }
        let filled = (self.done * look::BAR as u64 / self.total) as usize;
        let bar: String = std::iter::repeat_n(look::BAR_DONE, filled)
            .chain(std::iter::repeat_n(look::BAR_LEFT, look::BAR - filled))
            .collect();
        let size = |b: u64| render::value(&crate::Size(b).into(), &Ctx::plain());
        let line = format!(
            "\r\x1b[2K[{}] {}%  {} of {}  {}",
            &bar[..look::BAR],
            self.done * 100 / self.total,
            size(self.done),
            size(self.total),
            self.what
        );
        let _ = self
            .err
            .lock()
            .map(|mut e| e.write_all(line.as_bytes()).and_then(|_| e.flush()));
    }
}

impl Drop for Bar {
    fn drop(&mut self) {
        let size = render::value(&crate::Size(self.done).into(), &Ctx::plain());
        let clear = if self.tty { "\r\x1b[2K" } else { "" };
        let _ = self
            .err
            .lock()
            .map(|mut e| writeln!(e, "{clear}{}: {size}", self.what));
    }
}

/// Lines that show as they come.
pub struct Live<'a> {
    out: &'a mut Out,
}

impl Live<'_> {
    /// One line: perhaps its source, then its words.
    pub fn line(&mut self, source: Option<&crate::Name>, text: impl Into<Value>) {
        let text = text.into();
        if self.out.is_json() {
            let mut line = serde_json::json!({ "text": text });
            if let Some(s) = source {
                line["source"] = serde_json::to_value(Value::Name(s.clone())).unwrap_or_default();
            }
            let doc = format!("{line}\n");
            self.out.write_out(&doc);
            return;
        }
        let shown = render::value(&text, &self.out.out_ctx);
        let line = match source {
            Some(s) => format!("{}  {shown}\n", s.as_str()),
            None => format!("{shown}\n"),
        };
        self.out.write_out(&line);
    }

    /// A line of an agent's log ([`crate::log`]): with `--json`, its
    /// record, as many times as it was said; else rendered, its agent's
    /// ID left out when it's `own`, the agent whose log it is.
    pub fn log(&mut self, shown: &crate::log::Shown, own: Option<&str>) {
        if self.out.is_json() {
            let doc = serde_json::to_string(&shown.entry).unwrap_or_default();
            for _ in 0..shown.times {
                self.out.write_out(&format!("{doc}\n"));
            }
            return;
        }
        let line = format!("{}\n", shown.render(own, &self.out.out_ctx));
        self.out.write_out(&line);
    }

    /// A piece of a line as it streams in (a model's answer): shown and
    /// flushed at once, cleaned but for its newlines and tabs.
    pub fn piece(&mut self, piece: &str) {
        let piece: String = piece
            .chars()
            .filter(|c| matches!(c, '\n' | '\t') || (!c.is_control() && !crate::invisible(*c)))
            .collect();
        self.out.write_out(&piece);
        let _ = self.out.out.lock().map(|mut o| o.flush());
    }

    /// A line another program wrote on its stderr, passed on as it came.
    pub fn relay_err(&mut self, line: &str) {
        let line = format!("{}\n", crate::clean(line));
        self.out.write_err(&line);
    }
}

/// The terminal, lent: see [`Out::hand_over`].
pub struct HandOver(());

pub use crate::ask::{Answer, Question};

#[cfg(test)]
mod tests;
