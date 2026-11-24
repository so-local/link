//! A turn's own measures, which only the harness can count: its tokens
//! and cost, its tool calls, how long until the model was asked, until
//! the answer's first piece went out, and until it was whole. Appended,
//! as OTLP, to the file Link names (`LNK_MEASURES`), which Link's
//! measure module reads as the agent's. Numbers only, derived from the
//! turn's events: never a word of them.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use link_measure_contract::client::{self as measure, Point, Report, Unit};
use tokio::sync::mpsc;

use crate::agent::{Agent, Thread};
use crate::event::{self, Event};
use crate::store::Store;

/// Where measures go: `LNK_MEASURES`, unless it's unset.
pub fn from_env() -> Option<PathBuf> {
    std::env::var_os(measure::ENV)
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
}

/// When a turn's moments came.
pub struct Clock {
    started: Instant,
    /// Nanoseconds since 1970, at the start.
    at: u64,
    asked: OnceLock<Instant>,
    first: Arc<OnceLock<Instant>>,
}

impl Clock {
    pub fn start() -> Self {
        Self {
            started: Instant::now(),
            at: measure::now(),
            asked: OnceLock::new(),
            first: Arc::default(),
        }
    }

    /// The model is about to be asked.
    pub fn asking(&self) {
        self.asked.get_or_init(Instant::now);
    }

    /// `pieces`, noting when the first goes through: a sender to use in
    /// its place, passing each on.
    pub fn watch(&self, pieces: mpsc::Sender<String>) -> mpsc::Sender<String> {
        let (tx, mut rx) = mpsc::channel::<String>(64);
        let first = self.first.clone();
        tokio::spawn(async move {
            while let Some(piece) = rx.recv().await {
                first.get_or_init(Instant::now);
                if pieces.send(piece).await.is_err() {
                    break;
                }
            }
        });
        tx
    }
}

/// The measures of a turn whose events are `events` (from the owner's
/// message on), that `failed` or not, timed by `clock`, over at `ended`.
pub fn of_turn(events: &[Event], clock: &Clock, ended: Instant, failed: bool) -> Vec<Point> {
    let now = measure::now();
    let delta = |name: &str, value: f64| Point::new(name, value, now).since(clock.at);
    let seconds =
        |at: Option<&Instant>| at.map(|at| at.duration_since(clock.started).as_secs_f64());
    let mut points = vec![delta("link.turns.sum", 1.0)];
    let tools: Vec<&Event> = events.iter().filter(|e| e.role == event::TOOL).collect();
    let answers: Vec<&Event> = events
        .iter()
        .filter(|e| e.role == event::ASSISTANT || e.role == event::SUMMARY)
        .collect();
    points.push(delta("link.tool_calls.sum", tools.len() as f64));
    let errors = tools.iter().filter(|e| e.failed).count() + usize::from(failed);
    points.push(delta("link.errors.sum", errors as f64));
    // Answered by its fallback model, by why: a turn once each.
    for why in [
        crate::fallback::Why::Off,
        crate::fallback::Why::Window,
        crate::fallback::Why::Overloaded,
    ] {
        if answers
            .iter()
            .any(|e| e.fallback.as_deref() == Some(why.name()))
        {
            points.push(delta(&format!("link.fallbacks.{}.sum", why.name()), 1.0));
        }
    }
    let tokens = answers.iter().filter_map(|e| e.tokens.as_ref());
    let (mut input, mut output, mut cached) = (0, 0, 0);
    for t in tokens {
        input += t.input;
        output += t.output;
        cached += t.cached;
    }
    points.push(delta("link.tokens.input.sum", input as f64));
    points.push(delta("link.tokens.output.sum", output as f64));
    points.push(delta("link.tokens.cached.sum", cached as f64));
    // What the provider said it cost, or its count at the model's price.
    let costs: Vec<f64> = answers.iter().filter_map(|e| e.cost).collect();
    if !costs.is_empty() {
        points.push(delta("link.cost.sum", costs.iter().sum::<f64>() * 1e6));
    }
    if let Some(s) = seconds(clock.asked.get()) {
        points.push(Point::new("link.time.to_model.own", s, now));
    }
    if let Some(s) = seconds(clock.first.get()) {
        points.push(Point::new("link.time.first_token.own", s, now));
    }
    if !failed {
        points.push(Point::new(
            "link.time.turn.own",
            ended.duration_since(clock.started).as_secs_f64(),
            now,
        ));
    }
    points
}

/// A turn's measures, kept to `path` once it ends, whichever way: an
/// error, its lease lost, or dropped mid-way, as failed.
pub struct Measuring {
    pub clock: Clock,
    path: PathBuf,
    store: Arc<dyn Store>,
    agent: Agent,
    thread: Thread,
    /// The number of its first event, once it's read.
    first: AtomicU64,
    /// The number of the last event it wrote itself: a later turn's come
    /// after it, and aren't its own.
    last: AtomicU64,
    /// When its answer was whole, before its tools were let go.
    ended: Mutex<Option<Instant>>,
    done: bool,
}

impl Measuring {
    pub fn new(path: PathBuf, store: Arc<dyn Store>, agent: Agent, thread: Thread) -> Self {
        Self {
            clock: Clock::start(),
            path,
            store,
            agent,
            thread,
            first: AtomicU64::new(0),
            last: AtomicU64::new(0),
            ended: Mutex::default(),
            done: false,
        }
    }

    /// Its events start at `seq`, unless they started before.
    pub fn starts(&self, seq: u64) {
        let _ = self
            .first
            .compare_exchange(0, seq, Ordering::Relaxed, Ordering::Relaxed);
    }

    /// It wrote event `seq`.
    pub fn wrote(&self, seq: u64) {
        self.last.fetch_max(seq, Ordering::Relaxed);
    }

    /// Its answer is whole.
    pub fn ended(&self) {
        *self.ended.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    }

    /// It ended, `failed` or not: its measures kept.
    pub fn end(mut self, failed: bool) {
        self.keep(failed);
    }

    /// Its measures from its events, kept off the answer's way.
    fn keep(&mut self, failed: bool) {
        self.done = true;
        let first = self.first.load(Ordering::Relaxed);
        let last = self.last.load(Ordering::Relaxed);
        let ended = self
            .ended
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .unwrap_or_else(Instant::now);
        let clock = std::mem::replace(&mut self.clock, Clock::start());
        let (path, store) = (self.path.clone(), self.store.clone());
        let (agent, thread) = (self.agent.clone(), self.thread.clone());
        let keep = move || {
            // Read off the answer's way, the next turn may have written
            // by then: only its own, from its first to the last it wrote.
            // None read or written: nothing of its own to measure.
            let events = match (first, last) {
                (0, _) | (_, 0) => Vec::new(),
                (first, last) => {
                    let mut events = store.read_from(&agent, &thread, first).unwrap_or_default();
                    events.retain(|e| e.seq <= last);
                    events
                }
            };
            write(&path, &agent, of_turn(&events, &clock, ended, failed));
        };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => drop(runtime.spawn_blocking(keep)),
            Err(_) => keep(),
        }
    }
}

impl Drop for Measuring {
    fn drop(&mut self) {
        if !self.done {
            self.keep(true);
        }
    }
}

/// Appends `points` of `agent`'s turn to `path`, as OTLP, under its ID.
pub fn write(path: &Path, agent: &Agent, points: Vec<Point>) {
    let unit = Unit {
        id: agent.id().to_string(),
        kind: "agent".into(),
        name: Some(agent.name().to_string()),
        ..Unit::default()
    };
    let request = measure::otlp(&[Report { unit, points }], "lnk-harness");
    if let Err(err) = measure::append(path, &request) {
        crate::warn!("keeping a turn's measures: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agents, Folder};
    use crate::event::Tokens;
    use crate::store::Files;
    use link_test_kit::TempDir;

    /// An answer of `output` tokens, numbered `seq`.
    fn answer(seq: u64, output: u64) -> Event {
        Event {
            seq,
            role: event::ASSISTANT.into(),
            text: "Done.".into(),
            tokens: Some(Tokens {
                input: 1,
                output,
                cached: 0,
            }),
            ..Event::default()
        }
    }

    /// The next turn writes before the first's measures are read: they
    /// count the first's answer, never the next's.
    #[test]
    fn a_turns_measures_are_its_own() {
        let root = TempDir::new("measures-own");
        let agent = Agent::new("main").unwrap();
        let mut agents = Agents::default();
        agents.add(
            agent.clone(),
            root.join("home"),
            Folder {
                home: root.join("files"),
                agent: root.join("own"),
            },
        );
        let store: Arc<dyn Store> = Arc::new(Files::new(agents));
        let thread = Thread::new("telegram:42").unwrap();
        let path = root.join("measures.jsonl");
        let said = |seq, text| Event {
            seq,
            ..Event::user(text)
        };

        let first = Measuring::new(path.clone(), store.clone(), agent.clone(), thread.clone());
        store.append(&agent, &thread, &said(1, "Hi."), &[]).unwrap();
        first.starts(1);
        store.append(&agent, &thread, &answer(2, 5), &[]).unwrap();
        first.wrote(2);
        first.ended();
        // The next turn, whole, before the first's measures are read.
        store
            .append(&agent, &thread, &said(3, "Again."), &[])
            .unwrap();
        store.append(&agent, &thread, &answer(4, 7), &[]).unwrap();
        // Off a runtime, read and kept at once.
        first.end(false);

        let text = std::fs::read_to_string(&path).unwrap();
        let reports: Vec<_> = text
            .lines()
            .flat_map(|line| measure::read_otlp(&serde_json::from_str(line).unwrap()))
            .collect();
        let output: Vec<f64> = reports
            .iter()
            .flat_map(|r| &r.points)
            .filter(|p| p.measure.name == "link.tokens.output.sum")
            .map(|p| p.value)
            .collect();
        assert_eq!(output, [5.0]);
    }
}
