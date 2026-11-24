//! Link scheduler: messages for an agent from someone who isn't its
//! owner, now or later. A subagent is a message to a new conversation of
//! the agent's, now, whose answer comes back to the one that started it;
//! a schedule is a message to a conversation later, once or every so
//! often. They're one thing, so one tool and one runner:
//!
//! - **The tool** ([`mcp`]): an MCP server the agent calls, `task`,
//!   `schedule`, `send` (to a teammate), `message` (to a retained child),
//!   `list`, `read` and `cancel`, writing jobs to the store.
//! - **The runner** ([`run`]): fires each job when it's due, through a
//!   [`Driver`]: in this process ([`InProcess`], for tests), or the
//!   agent's bridge's way in ([`WayIn`]), the one that runs for good,
//!   restarting with the machine; or woken once a minute by the machine's
//!   calendar ([`Cron`], [`calendar`]), firing what's due and stopping; or
//!   on a machine that runs the agent only while it answers, told when
//!   the jobs are next due and woken then ([`Addressed`], [`Address`]). A
//!   message to a teammate goes to its bridge's way in ([`Ways`]), and its
//!   answer comes back as a job of its own ([`Kind::Reply`]).
//!
//! **The store** ([`Store`]): its jobs kept by a [`JobStore`], a folder
//! of the agent's unless the machine gives another, the store bridge on
//! a machine that runs the agent only while it answers ([`store`]), each kept
//! whole, so the tool and the runner, two processes, never see half of
//! one; a job being fired, moved aside until its firing ends; and the
//! day's count of messages sent.
//!
//! **Bounded** ([`Caps`]): so many messages a day, so many subagents at
//! once, and so many hops in a chain, each message carrying its count.
//! Hops bound how deep a chain goes; how many messages agents send each
//! other, the day's cap does. That they're bounded is decided here; the
//! numbers are the agent's spec's (`[tool.scheduler]`).

pub mod calendar;
pub mod cli;
mod clock;
mod constants;
pub use link_scheduler_tool_contract::cron;
mod drivers;
mod interactions;
mod keeping;
mod limits;
pub mod mcp;
pub mod portable;
mod relay;
mod runner;
pub mod store;
use clock::{local_at, local_day, local_offset, offset_text, zone_said};
pub use clock::{now, set_zone};
pub use drivers::{Address, Addressed, Caller, Cron, Driver, InProcess, WayIn, Ways};
pub use keeping::{Dependencies, Store};
pub use limits::{Limits, limits};
pub use relay::{InstallationTokens, Relayed, TeamRelay, Tokens};
pub use runner::{Looked, look, run, run_while, wake};

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};
use link_connection_contract::{AnswerTo, Fired, Firing};
use link_format::{Count, Elapsed, Foreign, Name, Sentence, sentence};
use serde::{Deserialize, Serialize};
pub use store::{JobStore, Place};

/// A warning the runner logs along the way: a daemon has no `Out`.
pub fn warn(headline: Sentence) {
    link_format::daemon::warn!("{headline}");
}

/// [`warn`], and the error's words after it.
pub fn warn_why(headline: Sentence, err: &anyhow::Error) {
    link_format::daemon::warn!("{headline} {err:#}");
}

/// A message for later, or now.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Job {
    /// Exact source occurrence for tool-created schedules, retained across delivery retries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence: Option<link_agent_state_protocol::schedules::ScheduledOccurrence>,
    /// The portable definition and exact occurrence, when projected from a spec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portable: Option<portable::Planned>,
    pub id: String,
    pub kind: Kind,
    /// The conversation it goes to.
    pub thread: String,
    pub text: String,
    /// Where its answer goes.
    pub answer: AnswerTo,
    /// When it's next due, in seconds since 1970.
    pub due: u64,
    /// Every so often, as cron says; none: once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    /// How many messages led to it.
    #[serde(default)]
    pub hops: u32,
    /// When it was made.
    pub created: u64,
    /// Its firings that failed, in a row: tried again up to [`Limits::retries`].
    #[serde(default)]
    pub tries: u32,
    /// A subagent's tools: its parent's, or fewer. None: its conversation
    /// keeps what it has (every tool, from a harness that can't narrow
    /// one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    /// The most it stands when it fires ([`Firing::standing`]): the
    /// standing of the conversation that made it, read from its call,
    /// never from what the model says. A teammate's message stands as a
    /// member, and a schedule from the agent's spec as its owner.
    pub standing: link_connection_contract::access::Standing,
    /// A subagent's model, a model or an alias: the one it was started
    /// with, else its parent's conversation's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// A message to a teammate ([`Kind::Send`]): the teammate's agent ID,
    /// and this agent's, who it's from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<Peers>,
    /// The task that started it, its coordinator, by its ID: none for a
    /// job its agent's own conversation started. Cancelling the
    /// coordinator cancels it ([`Store::cancel`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The jobs it waits on, by their IDs: it's fired once each ended
    /// done, and cancelled once one failed or was cancelled.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
    /// A subagent's short description, as its `task` call gave it, which
    /// `list` shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// How it ended, once it has: kept ended ([`Place::Ended`]) for the
    /// jobs that wait on it and whoever reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
}

/// How a job ended, and when, in seconds since 1970.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub at: u64,
    #[serde(flatten)]
    pub how: How,
}

/// How a job ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "ended")]
pub enum How {
    /// Its answer, at most [`Limits::result_max`] bytes of it.
    Done { answer: String },
    /// Given up on, after its tries.
    Failed { why: String },
    /// Cancelled: by its conversation, or with its coordinator or a job
    /// it waited on.
    Cancelled { why: String },
}

impl How {
    /// Done, with `answer` cut to [`Limits::result_max`] bytes at a character's
    /// end.
    pub fn done(answer: &str) -> How {
        let mut end = answer.len().min(limits().result_max);
        while !answer.is_char_boundary(end) {
            end -= 1;
        }
        How::Done {
            answer: answer[..end].to_string(),
        }
    }
}

/// Who a message between teammates is from and for, by their agents'
/// IDs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peers {
    pub from: String,
    pub to: String,
}

/// What a job is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A message to an existing retained child, without creating another task.
    ChildMessage,
    /// A subagent: a new conversation, its answer back to the one that
    /// started it.
    Task,
    /// A message to a conversation at a time, or every so often.
    Schedule,
    /// A message to a teammate, now: to its conversation with this agent
    /// (or the team's), its answer back to the one it was sent from.
    Send,
    /// A teammate's answer, to the conversation its message was sent
    /// from, as a message from it.
    Reply,
    /// A conversation's idle compaction (`[window] compact_after`): one
    /// a conversation, kept by its harness and moved with each message,
    /// which compacts in place of answering it
    /// ([`link_scheduler_tool_contract::compaction`]). Never the model's
    /// to list, read or cancel, and never one of the day's messages.
    Compact,
}

impl Kind {
    /// Whether a job of it is the model's own, to list, read and cancel,
    /// and one of the day's messages: every kind but [`Kind::Compact`].
    pub fn is_models(self) -> bool {
        self != Kind::Compact
    }

    /// Whether a job of it is one of the day's messages
    /// (`[tool.scheduler] per_day`): a schedule's, and a message between
    /// teammates either way. A subagent's work, and a message to a child,
    /// are bounded by how many run at once and how deep a chain goes,
    /// never by the day's budget, so a busy schedule can't starve them.
    pub fn counts(self) -> bool {
        match self {
            Kind::Schedule | Kind::Send | Kind::Reply => true,
            Kind::Task | Kind::ChildMessage | Kind::Compact => false,
        }
    }
}

impl Job {
    /// Who its message is from: `task:<id>` or `schedule:<id>`; a
    /// teammate's, `agent:<id>` (this agent, sending; the teammate,
    /// replying).
    pub fn from(&self) -> String {
        match (self.kind, &self.to) {
            (Kind::Send, Some(peers)) => format!("agent:{}", peers.from),
            (Kind::Reply, Some(peers)) => format!("agent:{}", peers.to),
            (Kind::Task | Kind::ChildMessage, _) => format!("task:{}", self.id),
            _ => format!("schedule:{}", self.id),
        }
    }

    /// Its message, as the bridge's way in takes it. A teammate's answer
    /// isn't waited for by anyone there: it comes back as a job of its
    /// own ([`Job::reply`]).
    pub fn firing(&self) -> Firing {
        let occurrence = self
            .portable
            .as_ref()
            .map(|p| p.occurrence.clone())
            .or_else(|| self.occurrence.clone());
        let isolated = self.kind == Kind::Schedule && occurrence.is_some();
        Firing {
            delivery: Some(
                link_interactions::TransportEventId::new(format!(
                    "job:{}:{}",
                    self.id,
                    occurrence.as_ref().map_or(self.created, |o| o.scheduled_at)
                ))
                .expect("source delivery identity"),
            ),
            occurrence: occurrence.clone(),
            // A subagent's lineage: the conversation that started it, which
            // is where its answer goes.
            parent: match (self.kind, &self.answer) {
                (Kind::Task, AnswerTo::Thread(parent)) => Some(parent.clone()),
                _ => None,
            },
            thread: if isolated {
                format!(
                    "schedule-{}-{}",
                    self.id,
                    occurrence
                        .as_ref()
                        .expect("isolated occurrence")
                        .scheduled_at
                )
            } else {
                self.thread.clone()
            },
            from: self.from(),
            text: self.text.clone(),
            answer: match self.kind {
                Kind::Send => AnswerTo::Nowhere,
                Kind::Schedule if isolated && self.answer == AnswerTo::Chat => {
                    AnswerTo::OwnerThread(self.thread.clone())
                }
                _ => self.answer.clone(),
            },
            hops: self.hops,
            tools: self.tools.clone(),
            model: self.model.clone(),
            standing: self.standing,
        }
    }

    /// The teammate's answer `answer` to it, a message sent, as a job:
    /// to the conversation it was sent from, now, one hop further. Its
    /// answer there goes to that conversation's chat; a conversation with
    /// none (another teammate's, a subagent's) keeps it, or its bridge
    /// sends it where it's owed.
    pub fn reply(&self, id: &str, answer: &str, at: u64) -> Option<Job> {
        let (Kind::Send, AnswerTo::Thread(back)) = (self.kind, &self.answer) else {
            return None;
        };
        Some(Job {
            occurrence: None,
            portable: None,
            id: id.to_string(),
            kind: Kind::Reply,
            thread: back.clone(),
            text: answer.to_string(),
            answer: match back.contains(':') {
                true => AnswerTo::Chat,
                false => AnswerTo::Nowhere,
            },
            due: at,
            cron: None,
            hops: self.hops.saturating_add(1),
            created: at,
            tries: 0,
            tools: None,
            standing: self.standing,
            model: None,
            to: self.to.clone(),
            parent: None,
            after: Vec::new(),
            description: None,
            outcome: None,
        })
    }

    /// Whether its outcome is kept once it ends: a delegation's (a
    /// subagent, a message to a child or a teammate), which others may
    /// wait on.
    pub fn keeps_outcome(&self) -> bool {
        matches!(self.kind, Kind::Task | Kind::ChildMessage | Kind::Send)
    }

    /// It, at its next time after `at`, its tries forgotten; none if it
    /// was due once.
    fn moved_on(&self, at: u64) -> Option<Job> {
        if let Some(planned) = &self.portable {
            let due = portable::next(&planned.definition.when, at).ok()??;
            let mut job = self.clone();
            job.due = due;
            job.tries = 0;
            job.portable.as_mut()?.occurrence.scheduled_at = due;
            return Some(job);
        }
        let due = cron::Cron::parse(self.cron.as_deref()?)
            .ok()?
            .next_after(at)?;
        Some(Job {
            due,
            tries: 0,
            occurrence: self.occurrence.as_ref().map(|o| {
                link_agent_state_protocol::schedules::ScheduledOccurrence {
                    scheduled_at: due,
                    ..o.clone()
                }
            }),
            ..self.clone()
        })
    }
}

/// The agent's teams, as the harness module hands them: who it is, and
/// whom it may send to. Empty: it's on no team, and has no `send`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Teams {
    /// The agent's own ID: who its messages are from.
    pub agent: Option<String>,
    /// Its teams' IDs, by name.
    pub teams: std::collections::BTreeMap<String, String>,
    /// The others on them.
    pub teammates: Vec<Teammate>,
}

/// Another member of one of the agent's teams.
#[derive(Clone, Debug, PartialEq)]
pub struct Teammate {
    /// The team's name, and the member's name in it.
    pub team: String,
    pub name: String,
    /// Its agent's ID.
    pub agent: String,
}

impl Teams {
    /// `--team-id <name>=<id>` and `--teammate <team>/<name>=<agent id>`:
    /// each checked, a teammate's team given, and never the agent itself.
    pub fn add_team(&mut self, arg: &str) -> anyhow::Result<()> {
        let (name, id) = arg
            .split_once('=')
            .context("--team-id is <name>=<id>: research=t4m9q2x7k1ab")?;
        link_machine::machine::check_name(name)?;
        link_machine::machine::check_id(id)?;
        self.teams.insert(name.to_string(), id.to_string());
        Ok(())
    }

    pub fn add_teammate(&mut self, arg: &str) -> anyhow::Result<()> {
        let bad = || format!("--teammate is <team>/<name>=<agent id>, not {arg:?}");
        let (who, agent) = arg.split_once('=').with_context(bad)?;
        let (team, name) = who.split_once('/').with_context(bad)?;
        link_machine::machine::check_name(team)?;
        link_machine::machine::check_name(name)?;
        link_machine::machine::check_id(agent)?;
        self.teammates.push(Teammate {
            team: team.to_string(),
            name: name.to_string(),
            agent: agent.to_string(),
        });
        Ok(())
    }

    /// An error for a teammate of a team not given, or that is the agent
    /// itself, or teammates without the agent's ID.
    pub fn check(&self) -> anyhow::Result<()> {
        if self.teammates.is_empty() && self.teams.is_empty() {
            return Ok(());
        }
        let me = self
            .agent
            .as_deref()
            .context("--team-id and --teammate need --agent-id, the agent's own ID")?;
        link_machine::machine::check_id(me)?;
        for mate in &self.teammates {
            anyhow::ensure!(
                self.teams.contains_key(&mate.team),
                "--teammate {}/{}: no --team-id {}",
                mate.team,
                mate.name,
                mate.team
            );
            anyhow::ensure!(
                mate.agent != me,
                "--teammate {}/{} is the agent itself",
                mate.team,
                mate.name
            );
        }
        Ok(())
    }
}

/// The conversation a teammate's messages to an agent land in, there:
/// one for each teammate (`agent-<its ID>`), and one for a whole team's
/// (`team-<its ID>`).
pub fn teammates_thread(from: &str) -> String {
    link_connection_contract::teammates_thread(from)
}

pub fn teams_thread(team: &str) -> String {
    link_connection_contract::teams_thread(team)
}

/// The caps, from the agent's spec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    pub per_day: u32,
    pub subagents: u32,
    pub hops: u32,
    /// A firing missed while the machine slept runs once on waking.
    pub late_runs: bool,
    /// The shortest a recurring schedule may repeat (`--interval`): a
    /// cron firing more often is refused when it's made; none, any.
    pub interval: Option<Duration>,
    /// How long the runner waits on a firing before it gives up on it
    /// (it's then never tried again, and its subagent counts until it
    /// ends); none, the tool's, which fires nothing.
    pub timeout: Option<Duration>,
}

#[cfg(test)]
mod delegation_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use link_test_kit::TempDir;
    use std::sync::{Arc, Mutex};

    pub(crate) fn store(name: &str) -> (TempDir, Store) {
        let dir = TempDir::new(&format!("sched-{name}"));
        let store = Store::new(&dir).unwrap();
        (dir, store)
    }

    pub(crate) fn job(id: &str, due: u64) -> Job {
        Job {
            occurrence: None,
            portable: None,
            model: None,
            id: id.into(),
            kind: Kind::Schedule,
            thread: "telegram:42".into(),
            text: "Good morning.".into(),
            answer: AnswerTo::Chat,
            due,
            cron: None,
            hops: 1,
            created: 0,
            tries: 0,
            tools: None,
            standing: link_connection_contract::access::Standing::Owner,
            to: None,
            parent: None,
            after: Vec::new(),
            description: None,
            outcome: None,
        }
    }

    pub(crate) const CAPS: Caps = Caps {
        per_day: 100,
        subagents: 5,
        hops: 8,
        late_runs: true,
        timeout: None,
        interval: None,
    };

    pub(crate) fn recording() -> (Arc<dyn Driver>, Arc<Mutex<Vec<Firing>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        let driver: Arc<dyn Driver> = Arc::new(InProcess(move |firing: &Firing| {
            kept.lock().unwrap().push(firing.clone());
            Ok(link_connection_contract::Fired {
                answer: "ok".into(),
            })
        }));
        (driver, seen)
    }

    /// A driver whose first `failures` firings fail, as a bridge not up
    /// yet does: how many it was asked.
    pub(crate) fn failing(failures: usize) -> (Arc<dyn Driver>, Arc<Mutex<usize>>) {
        let asked = Arc::new(Mutex::new(0));
        let kept = asked.clone();
        let driver: Arc<dyn Driver> = Arc::new(InProcess(move |_: &Firing| {
            let mut asked = kept.lock().unwrap();
            *asked += 1;
            match *asked <= failures {
                true => bail!("reaching the agent's bridge: no such file"),
                false => Ok(link_connection_contract::Fired::default()),
            }
        }));
        (driver, asked)
    }

    /// How a test's runner wakes: resident, looking when told ([`look`]),
    /// or as the calendar wakes it ([`wake`], [`Cron`]).
    #[derive(Clone, Copy, Debug)]
    pub(crate) enum Wakes {
        Here,
        Cron,
    }

    /// Every driver's runner: each test that says so passes on both,
    /// whatever reaches the agent (in process, or the way in).
    pub(crate) const EVERY_DRIVER: [Wakes; 2] = [Wakes::Here, Wakes::Cron];

    /// One look at `at`, its firings ended, as a runner woken by `wakes`
    /// takes it: through `driver`, or the cron driver holding it.
    pub(crate) fn fire_at(
        wakes: Wakes,
        store: &Store,
        driver: &Arc<dyn Driver>,
        caps: &Caps,
        at: u64,
    ) -> Looked {
        match wakes {
            Wakes::Here => look(store, driver, caps, at, true),
            Wakes::Cron => {
                let cron: Arc<dyn Driver> = Arc::new(Cron(driver.clone()));
                wake(store, &cron, caps, at).unwrap()
            }
        }
    }
}

#[cfg(test)]
link_test_kit::program!(system);
