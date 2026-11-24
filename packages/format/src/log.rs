//! An agent's log: what its harness and Link say about its run, one
//! [`Line`] a line, each with its time in UTC and the IDs that place it
//! (its agent, conversation, turn and tool call). A process whose stderr
//! is a log ([`to_stderr`]) writes records there through [`say`]; any
//! other shows the same words as a block on its terminal ([`aside`]).
//!
//! ```text
//! {"at":"2026-11-09T14:05:11.042Z","level":"warning","agent":"3e75d6ac…","chat":"link:3e75d6ac…","call":"toolu_01…","text":"tool agent__send failed: …","schema":1}
//! ```
//!
//! A person reads it through [`Shown`]: its local time, a mark for its
//! level, its words and its IDs shortened and faint, the same line
//! written again and again shown once with how many times
//! ([`collapse`]).

use std::io::Write;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::look::Mark;
use crate::render::Ctx;
use crate::{Note, Report, Sentence, aside};

/// How much a line matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// About the run: it started, it answers on a port.
    Info,
    /// Something went wrong that the run goes on after.
    Warning,
    /// Something went wrong that ended what it was doing.
    Error,
}

/// When a line was written: RFC 3339 in UTC, to the millisecond.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp(pub SystemTime);

impl Serialize for Stamp {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&crate::time::rfc3339_millis(self.0))
    }
}

impl<'de> Deserialize<'de> for Stamp {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        crate::time::parse_rfc3339(&text)
            .map(Stamp)
            .ok_or_else(|| serde::de::Error::custom(format!("{text:?} isn't an RFC 3339 time")))
    }
}

/// The IDs that place a line, each where it has one: the agent's, its
/// conversation's (a chat, as `link:3e75d6ac…`), its turn's and its tool
/// call's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ids {
    pub agent: Option<String>,
    pub chat: Option<String>,
    pub turn: Option<String>,
    pub call: Option<String>,
}

impl Ids {
    /// Agent `id`'s.
    pub fn agent(id: impl Into<String>) -> Self {
        Ids {
            agent: Some(id.into()),
            ..Ids::default()
        }
    }

    /// In conversation `chat`.
    pub fn chat(mut self, chat: impl Into<String>) -> Self {
        self.chat = Some(chat.into());
        self
    }

    /// In turn `turn`.
    pub fn turn(mut self, turn: impl Into<String>) -> Self {
        self.turn = Some(turn.into());
        self
    }

    /// About tool call `call`.
    pub fn call(mut self, call: impl Into<String>) -> Self {
        self.call = Some(call.into());
        self
    }
}

/// One line of an agent's log: when, how much it matters, the IDs that
/// place it and its words.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Line {
    pub at: Stamp,
    pub level: Level,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<String>,
    pub text: String,
}

link_files::specification!(Line, include_str!("log.schema.toml"));

impl Line {
    /// `text`, said now at `level`, placed by `ids`.
    pub fn now(level: Level, text: &str, ids: Ids) -> Self {
        Line {
            at: Stamp(SystemTime::now()),
            level,
            agent: ids.agent,
            chat: ids.chat,
            turn: ids.turn,
            call: ids.call,
            text: text.to_string(),
        }
    }

    /// Its IDs.
    pub fn ids(&self) -> Ids {
        Ids {
            agent: self.agent.clone(),
            chat: self.chat.clone(),
            turn: self.turn.clone(),
            call: self.call.clone(),
        }
    }

    /// Written to `to` as one line, naming its schema, in one write.
    pub fn write(&self, to: &mut impl Write) -> std::io::Result<()> {
        let line = link_files::saved::line(self).map_err(std::io::Error::other)?;
        to.write_all(&line)
    }

    /// The record `line` holds (its newline left off); `None` for a line
    /// that isn't one.
    pub fn parse(line: &str) -> Option<Line> {
        link_files::saved::parse_line(line.trim_end().as_bytes()).ok()
    }
}

/// Whether this process's stderr is a log ([`to_stderr`]).
static TO_STDERR: AtomicBool = AtomicBool::new(false);

/// The agent this process's lines are about when [`say`] names none.
static AGENT: RwLock<Option<String>> = RwLock::new(None);

/// From now on, [`say`] writes records to stderr, about agent `agent`
/// when a line names none: for a process whose stderr is its log.
pub fn to_stderr(agent: Option<&str>) {
    TO_STDERR.store(true, Ordering::Relaxed);
    if let Some(agent) = agent {
        set_agent(agent);
    }
}

/// The agent this process's lines are about from now on, when one names
/// none: the agent a harness started blank was bound to.
pub fn set_agent(agent: &str) {
    if let Ok(mut held) = AGENT.write() {
        *held = Some(agent.to_string());
    }
}

/// Says `text` at `level`, placed by `ids`: a record on stderr, if it's
/// a log ([`to_stderr`]), else a block [`aside`].
pub fn say(level: Level, text: &str, mut ids: Ids) {
    if !TO_STDERR.load(Ordering::Relaxed) {
        let sentence = Sentence::adopt(text);
        return match level {
            Level::Info => aside(Note(sentence)),
            Level::Warning => aside(Report::warning(sentence)),
            Level::Error => aside(Report::error(sentence)),
        };
    }
    if ids.agent.is_none() {
        ids.agent = AGENT.read().ok().and_then(|agent| agent.clone());
    }
    let _ = Line::now(level, text, ids).write(&mut std::io::stderr().lock());
}

/// A line of an agent's log as it's read: Link's record, or a line its
/// harness wrote itself (one that isn't Link's), kept as it came.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Entry {
    Line(Line),
    Said { text: String },
}

impl Entry {
    /// `line`, without its newline, as one entry; a harness's own line
    /// cleaned of control characters.
    pub fn read(line: &str) -> Entry {
        match Line::parse(line) {
            Some(line) => Entry::Line(line),
            None => Entry::Said {
                text: crate::clean(&line.replace('\t', "    ")),
            },
        }
    }

    /// Whether it says what `other` says, about the same things, whenever
    /// each was written.
    fn same(&self, other: &Entry) -> bool {
        match (self, other) {
            (Entry::Line(a), Entry::Line(b)) => {
                a.level == b.level && a.text == b.text && a.ids() == b.ids()
            }
            (Entry::Said { text: a }, Entry::Said { text: b }) => a == b,
            _ => false,
        }
    }
}

/// An entry as a person reads it: the first of `times` the same in a
/// row.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub entry: Entry,
    pub times: usize,
}

/// `entries`, each run of the same entry ([`Entry::same`]) shown once
/// with how many it was.
pub fn collapse(entries: impl IntoIterator<Item = Entry>) -> Vec<Shown> {
    let mut shown: Vec<Shown> = Vec::new();
    for entry in entries {
        match shown.last_mut() {
            Some(last) if last.entry.same(&entry) => last.times += 1,
            _ => shown.push(Shown { entry, times: 1 }),
        }
    }
    shown
}

/// How long an ID part shows before it's cut.
const ID_SHOWN: usize = 8;

/// `id` short: each part (between `:`) longer than [`ID_SHOWN`] cut to
/// it, and `…` (`link:3e75d6ac…`).
fn short(id: &str) -> String {
    id.split(':')
        .map(|part| match part.chars().count() > ID_SHOWN + 1 {
            true => format!(
                "{}\u{2026}",
                part.chars().take(ID_SHOWN).collect::<String>()
            ),
            false => part.to_string(),
        })
        .collect::<Vec<_>>()
        .join(":")
}

impl Shown {
    /// One line, as `ctx` styles it, without its newline: its local time,
    /// its level's mark, its words, then its IDs short and faint (its
    /// agent's left out when it's `own`, the agent whose log it is) and
    /// how many times it was said in a row.
    pub fn render(&self, own: Option<&str>, ctx: &Ctx) -> String {
        let times = match self.times {
            1 => String::new(),
            n => format!("  {}", ctx.faint(&format!("\u{d7}{n}"))),
        };
        let line = match &self.entry {
            Entry::Said { text } => return format!("{text}{times}"),
            Entry::Line(line) => line,
        };
        let mark = match line.level {
            Level::Info => " ".to_string(),
            Level::Warning => ctx.mark(Mark::Warning),
            Level::Error => ctx.mark(Mark::Error),
        };
        let ids: Vec<String> = [
            (
                "agent",
                line.agent.as_deref().filter(|agent| Some(*agent) != own),
            ),
            ("chat", line.chat.as_deref()),
            ("turn", line.turn.as_deref()),
            ("call", line.call.as_deref()),
        ]
        .into_iter()
        .filter_map(|(kind, id)| id.map(|id| format!("{kind} {}", short(id))))
        .collect();
        let ids = match ids.is_empty() {
            true => String::new(),
            false => format!("  {}", ctx.faint(&ids.join(crate::look::SEPARATOR))),
        };
        format!(
            "{} {mark} {}{ids}{times}",
            ctx.faint(&crate::time::local_clock(line.at.0, ctx.now)),
            crate::clean(&line.text.replace(['\n', '\t'], " ")),
        )
    }
}

mod relay;
pub use relay::{LONGEST_LINE, Split, relay};

/// How [`Shown::render`] styles a line passed to `stream`: as the
/// output settings say on a terminal, plain elsewhere.
pub fn styled(stream: crate::streams::Stream) -> Ctx {
    let terminal = match stream {
        crate::streams::Stream::Out => crate::streams::stdout_is_terminal(),
        crate::streams::Stream::Err => crate::streams::stderr_is_terminal(),
    };
    match terminal {
        true => Ctx::terminal(
            &crate::look::Settings::from_env(),
            crate::out::terminal_width(),
        ),
        false => Ctx::plain(),
    }
}

#[cfg(test)]
mod tests;
