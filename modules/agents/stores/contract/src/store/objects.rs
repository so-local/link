//! The store contract over any bucket of versioned objects
//! ([`crate::files::Store`]): one agent's conversations, their leases and
//! narrowed tools, its jobs, and its files by path, each an object,
//! every change of one a write that names the version it read
//! ([`Expect`]). What the store promises follows from the bucket's: of
//! two writers of one version, it lets one write. The `bucket` place's
//! driver keeps an agent's collections this way, and the `file` place's
//! its files by path.
//!
//! The objects, under the agent's own bucket or folder:
//!
//! ```text
//! conversations/<thread>/events/<seq, 20 digits>   an event, as JSON
//! conversations/<thread>/lease                     its turn's lease, as JSON
//! conversations/<thread>/tools.json                the tools it was narrowed to
//! schedule/<zone>/<job>                           a job, as JSON
//! schedule/day                                     the day's count
//! schedule/mark                                    moved on with each change to the waiting jobs
//! <collection>/<path>                              a file by its path (instructions/instruction.md)
//! ```
//!
//! A thread's and a job's name is escaped: a byte but a letter, a digit,
//! `-` or `_` is `%` and its two hex digits. Each file object is a typed
//! record with a monotonically increasing revision and optional base64
//! payload. Removal keeps a tombstone so an old revision cannot return.
//! Backend object versions remain the compare-and-swap fence.

use std::time::Duration;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    Agent, FILES_MAX, Job, Kept, Lease, Place, Reply, Request, Stored, Took, check_file_path,
    check_job, check_job_id, check_tools, file_data, file_max, folder, seq_of,
};
use crate::files::{Expect, Store, Version, is_conflict};

mod files;
mod jobs;
pub use files::file_record_max;
#[cfg(all(test, feature = "files-bridge"))]
mod tests;

use jobs::{JobHeld, held_job};

/// Digits of an event's number in its object's name: a `u64`'s, so the
/// names sort as the numbers do.
const DIGITS: usize = 20;

/// How many times a change that lost a race is read and tried again
/// before it's given up: many writers at once each win in turn.
const TRIES: usize = 64;

/// One agent's collections as objects in `store`, which holds that
/// agent's alone: a call for another agent is refused.
pub struct Objects<S> {
    store: S,
    agent: Agent,
    /// Milliseconds since 1970, the clock leases are judged by.
    now: fn() -> u64,
}

/// What a lease object holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Held {
    holder: String,
    /// Its thread's name, as the lease was taken: what `leases` answers.
    thread: String,
    started: u64,
    renewed: u64,
    /// When it was renewed to the millisecond: a lease's lapse is
    /// judged by it, never by `renewed`'s whole second (up to a second
    /// early).
    renewed_ms: u64,
}

/// The day's count, as `schedule/day` holds it.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
struct Day {
    day: u64,
    sent: u32,
}

/// `name` escaped for an object's name: a byte but a letter, a digit,
/// `-` or `_` is `%` and its two hex digits.
pub fn escape(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// What [`escape`] made, back: none for what it never makes, so a name
/// reads back from one spelling only (its escapes in uppercase hex).
pub fn unescape(escaped: &str) -> Option<String> {
    let name = unescaped(escaped)?;
    (escape(&name) == escaped).then_some(name)
}

fn unescaped(escaped: &str) -> Option<String> {
    let mut out = Vec::with_capacity(escaped.len());
    let bytes = escaped.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = escaped.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// An object's version as the contract's number: the first eight bytes
/// of its SHA-256, never 0, which says there's none.
pub fn version_of(version: &Version) -> u64 {
    let mut first = [0u8; 8];
    first.copy_from_slice(&link_digest::sha256_bytes(version.as_bytes())[..8]);
    u64::from_be_bytes(first).max(1)
}

fn place_name(place: Place) -> &'static str {
    match place {
        Place::Waiting => "waiting",
        Place::Firing => "firing",
        Place::Ending => "ending",
        Place::Ended => "ended",
    }
}

impl<S: Store> Objects<S> {
    /// Agent `agent`'s collections in `store`.
    pub fn new(store: S, agent: Agent) -> Self {
        Self {
            store,
            agent,
            now: super::now_ms,
        }
    }

    /// The same, leases judged by `now`'s clock, in milliseconds (tests).
    pub fn with_clock(mut self, now: fn() -> u64) -> Self {
        self.now = now;
        self
    }

    /// The bucket it's kept in.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// One call, answered: an error to refuse it.
    pub fn answer(&self, request: &Request) -> anyhow::Result<Reply> {
        anyhow::ensure!(
            request.agent() == &self.agent,
            "agent {} asked for another agent's collections: refused",
            link_format::clean(&request.agent().id)
        );
        Ok(match request {
            Request::Append { thread, event, .. } => self.append(thread, event)?,
            Request::Read { thread, first, .. } => Reply::Events {
                events: self.read(thread, *first)?,
            },
            Request::Last { thread, .. } => Reply::Last {
                last: self.last(thread)?,
            },
            Request::Threads { .. } => Reply::Threads {
                threads: self.threads()?,
            },
            Request::Remove { thread, .. } => {
                // Its events and lease; the tools it was narrowed to stay,
                // so it's never made again with more.
                let tools = format!("{}{}", self.conversation(thread), folder::TOOLS);
                for object in self.store.list(&self.conversation(thread))? {
                    if object.path != tools {
                        self.store.delete(&object.path, &Expect::Any)?;
                    }
                }
                Reply::Done
            }
            Request::Take {
                thread,
                holder,
                lapse_ms,
                ..
            } => Reply::Took {
                took: self.take(thread, holder, *lapse_ms)?,
            },
            Request::Renew { thread, holder, .. } => self.renew(thread, holder)?,
            Request::End { thread, holder, .. } => {
                let name = format!("{}lease", self.conversation(thread));
                if let Some((bytes, version)) = self.store.get(&name)? {
                    let held: Held = serde_json::from_slice(&bytes)?;
                    if held.holder == *holder {
                        match self.store.delete(&name, &Expect::Version(version)) {
                            // Taken over meanwhile: not this holder's to end.
                            Err(err) if is_conflict(&err) => {}
                            other => other?,
                        }
                    }
                }
                Reply::Done
            }
            Request::Leases { lapse_ms, .. } => Reply::Leases {
                leases: self.leases(*lapse_ms)?,
            },
            Request::JobPut { place, job, .. } => {
                check_job(job)?;
                if let Some((bytes, _)) = self.store.get(&self.job(*place, &job.id))?
                    && matches!(held_job(&bytes)?, JobHeld::Job(_))
                {
                    self.finish_claim(&bytes)?;
                }
                self.store.put(
                    &self.job(*place, &job.id),
                    &serde_json::to_vec(job)?,
                    &Expect::Any,
                )?;
                self.moved(*place)?;
                Reply::Done
            }
            Request::JobGet { place, id, .. } => {
                check_job_id(id)?;
                Reply::Job {
                    job: self.job_at(*place, id)?.map(|(job, _)| job),
                }
            }
            Request::JobList { place, .. } => Reply::Jobs {
                jobs: self.jobs(*place)?,
            },
            Request::JobCount { place, .. } => Reply::Count {
                count: self.jobs(*place)?.len() as u64,
            },
            Request::JobShift { id, from, to, .. } => {
                check_job_id(id)?;
                Reply::Found {
                    found: self.shift(id, *from, *to)?,
                }
            }
            Request::JobRemove { place, id, .. } => {
                check_job_id(id)?;
                let name = self.job(*place, id);
                let found = match self.store.get(&name)? {
                    None => false,
                    Some((bytes, version)) if matches!(held_job(&bytes)?, JobHeld::Moving(_)) => {
                        self.settle_stopped(&name, &bytes, &version)?;
                        false
                    }
                    Some((bytes, version)) => {
                        self.finish_claim(&bytes)?;
                        match self.store.delete(&name, &Expect::Version(version)) {
                            Ok(()) => true,
                            Err(err) if is_conflict(&err) => false,
                            Err(err) => return Err(err),
                        }
                    }
                };
                if found {
                    self.moved(*place)?;
                }
                Reply::Found { found }
            }
            Request::JobNext { .. } => Reply::Next {
                next: self.jobs(Place::Waiting)?.iter().map(|j| j.due).min(),
            },
            Request::JobDay { .. } => {
                let day: Day = match self.store.get("schedule/day")? {
                    Some((bytes, _)) => serde_json::from_slice(&bytes)?,
                    None => Day::default(),
                };
                Reply::Day {
                    day: day.day,
                    sent: day.sent,
                }
            }
            Request::JobSetDay { day, sent, .. } => {
                let day = Day {
                    day: *day,
                    sent: *sent,
                };
                self.store
                    .put("schedule/day", &serde_json::to_vec(&day)?, &Expect::Any)?;
                Reply::Done
            }
            Request::JobMark { .. } => Reply::Mark {
                mark: self.mark()?.0,
            },
            Request::FileRead { path, .. } => {
                check_file_path(path, false)?;
                Reply::File {
                    file: self.file(path)?.map(|(bytes, version)| {
                        use base64::Engine as _;
                        Stored {
                            data: base64::engine::general_purpose::STANDARD.encode(bytes),
                            version,
                        }
                    }),
                }
            }
            Request::FileList { folder, .. } => {
                check_file_path(folder, true)?;
                let prefix = match folder.ends_with('/') {
                    true => folder.clone(),
                    false => format!("{folder}/"),
                };
                let mut files = Vec::new();
                for object in self.store.list(&prefix)? {
                    check_file_path(&object.path, false)?;
                    if self.file(&object.path)?.is_some() {
                        anyhow::ensure!(
                            files.len() < FILES_MAX,
                            "{} holds more than {FILES_MAX} files",
                            link_format::clean(folder)
                        );
                        files.push(object.path);
                    }
                }
                files.sort();
                Reply::Files { files }
            }
            Request::FileWrite {
                path,
                data,
                version,
                ..
            } => {
                check_file_path(path, false)?;
                self.write(path, &file_data(path, data)?, *version)?
            }
            Request::FileRemove { path, version, .. } => {
                check_file_path(path, false)?;
                let found = self.remove_file(path, *version)?;
                Reply::Found { found }
            }
            Request::FileAppend { path, data, .. } => {
                check_file_path(path, false)?;
                let data = file_data(path, data)?;
                Reply::Wrote {
                    version: self.append_file(path, &data)?,
                }
            }
            Request::ToolsRead { thread, .. } => Reply::Tools {
                tools: self.tools(thread)?.map(|(tools, _)| tools),
            },
            Request::ToolsList { .. } => {
                let mut narrowed = std::collections::BTreeMap::new();
                for object in self.store.list("conversations/")? {
                    let rest = &object.path["conversations/".len()..];
                    let Some((escaped, folder::TOOLS)) = rest.split_once('/') else {
                        continue;
                    };
                    let (Some(thread), Some((bytes, _))) =
                        (unescape(escaped), self.store.get(&object.path)?)
                    else {
                        continue;
                    };
                    narrowed.insert(thread, folder::read_tools(&bytes)?);
                }
                Reply::Narrowed { narrowed }
            }
            Request::ToolsNarrow { thread, tools, .. } => {
                check_tools(tools)?;
                let name = format!("{}{}", self.conversation(thread), folder::TOOLS);
                let mut narrowed = Vec::new();
                self.change(&name, |had| {
                    narrowed = match had {
                        Some(bytes) => {
                            let had = folder::read_tools(&bytes)?;
                            tools.iter().filter(|t| had.contains(t)).cloned().collect()
                        }
                        None => tools.clone(),
                    };
                    Ok(Some(folder::tools_file(&narrowed)?))
                })?;
                Reply::Tools {
                    tools: Some(narrowed),
                }
            }
        })
    }

    /// The prefix of conversation `thread`'s objects.
    fn conversation(&self, thread: &str) -> String {
        format!("conversations/{}/", escape(thread))
    }

    fn event(&self, thread: &str, seq: u64) -> String {
        format!("{}events/{seq:0DIGITS$}", self.conversation(thread))
    }

    /// The numbers of conversation `thread`'s events, in order.
    fn seqs(&self, thread: &str) -> anyhow::Result<Vec<u64>> {
        let prefix = format!("{}events/", self.conversation(thread));
        let mut seqs: Vec<u64> = self
            .store
            .list(&prefix)?
            .iter()
            .filter_map(|o| o.path[prefix.len()..].parse().ok())
            .collect();
        seqs.sort_unstable();
        Ok(seqs)
    }

    fn last(&self, thread: &str) -> anyhow::Result<u64> {
        Ok(self.seqs(thread)?.last().copied().unwrap_or(0))
    }

    fn append(&self, thread: &str, event: &Value) -> anyhow::Result<Reply> {
        let seq = seq_of(event).context("an event without its number")?;
        let last = self.last(thread)?;
        if seq != last + 1 {
            return Ok(Reply::Taken { last });
        }
        match self.store.put(
            &self.event(thread, seq),
            &serde_json::to_vec(event)?,
            &Expect::Absent,
        ) {
            Ok(_) => Ok(Reply::Done),
            Err(err) if is_conflict(&err) => Ok(Reply::Taken {
                last: self.last(thread)?,
            }),
            Err(err) => Err(err),
        }
    }

    fn read(&self, thread: &str, first: u64) -> anyhow::Result<Vec<Value>> {
        let mut events = Vec::new();
        for seq in self.seqs(thread)?.into_iter().filter(|s| *s >= first) {
            // Removed meanwhile: what's left is read.
            let Some((bytes, _)) = self.store.get(&self.event(thread, seq))? else {
                continue;
            };
            events.push(serde_json::from_slice(&bytes).context("reading an event")?);
        }
        Ok(events)
    }

    fn threads(&self) -> anyhow::Result<Vec<Kept>> {
        let mut kept: std::collections::BTreeMap<String, u64> = Default::default();
        for object in self.store.list("conversations/")? {
            let rest = &object.path["conversations/".len()..];
            let Some((escaped, event)) = rest.split_once("/events/") else {
                continue;
            };
            let (Some(thread), Ok(seq)) = (unescape(escaped), event.parse::<u64>()) else {
                continue;
            };
            let last = kept.entry(thread).or_default();
            *last = (*last).max(seq);
        }
        Ok(kept
            .into_iter()
            .map(|(thread, last)| Kept { thread, last })
            .collect())
    }

    fn take(&self, thread: &str, holder: &str, lapse_ms: u64) -> anyhow::Result<Took> {
        let name = format!("{}lease", self.conversation(thread));
        let now = (self.now)();
        let fresh = Held {
            holder: holder.to_string(),
            thread: thread.to_string(),
            started: now / 1000,
            renewed: now / 1000,
            renewed_ms: now,
        };
        let (expect, took) = match self.store.get(&name)? {
            None => (Expect::Absent, Took::Took),
            Some((bytes, version)) => {
                let held: Held = serde_json::from_slice(&bytes)?;
                if !lapsed(&held, now, lapse_ms) {
                    return Ok(Took::Held);
                }
                (Expect::Version(version), Took::Lapsed)
            }
        };
        match self.store.put(&name, &serde_json::to_vec(&fresh)?, &expect) {
            Ok(_) => Ok(took),
            // Another took it first.
            Err(err) if is_conflict(&err) => Ok(Took::Held),
            Err(err) => Err(err),
        }
    }

    fn renew(&self, thread: &str, holder: &str) -> anyhow::Result<Reply> {
        let name = format!("{}lease", self.conversation(thread));
        for _ in 0..TRIES {
            let Some((bytes, version)) = self.store.get(&name)? else {
                return Ok(Reply::Lost);
            };
            let mut held: Held = serde_json::from_slice(&bytes)?;
            if held.holder != holder {
                return Ok(Reply::Lost);
            }
            let now = (self.now)();
            held.renewed = now / 1000;
            held.renewed_ms = now;
            match self.store.put(
                &name,
                &serde_json::to_vec(&held)?,
                &Expect::Version(version),
            ) {
                Ok(_) => return Ok(Reply::Done),
                Err(err) if is_conflict(&err) => continue,
                Err(err) => return Err(err),
            }
        }
        bail!("its lease kept changing as it was renewed")
    }

    fn leases(&self, lapse_ms: u64) -> anyhow::Result<Vec<Lease>> {
        let now = (self.now)();
        let mut leases = Vec::new();
        for object in self.store.list("conversations/")? {
            if !object.path.ends_with("/lease") {
                continue;
            }
            let Some((bytes, _)) = self.store.get(&object.path)? else {
                continue;
            };
            let held: Held = serde_json::from_slice(&bytes)?;
            leases.push(Lease {
                lapsed: lapsed(&held, now, lapse_ms),
                thread: held.thread,
                holder: held.holder,
                started: held.started,
                renewed: held.renewed,
            });
        }
        leases.sort_by(|a, b| a.thread.cmp(&b.thread));
        Ok(leases)
    }

    fn tools(&self, thread: &str) -> anyhow::Result<Option<(Vec<String>, Version)>> {
        let name = format!("{}{}", self.conversation(thread), folder::TOOLS);
        match self.store.get(&name)? {
            None => Ok(None),
            Some((bytes, version)) => Ok(Some((folder::read_tools(&bytes)?, version))),
        }
    }

    /// Object `name` made what `change` makes of what it holds (none for
    /// none), written only at the version read, read again and changed
    /// again when another wrote first: its new version.
    #[expect(
        clippy::disallowed_methods,
        reason = "backs off after a conflicting write"
    )]
    fn change(
        &self,
        name: &str,
        mut change: impl FnMut(Option<Vec<u8>>) -> anyhow::Result<Option<Vec<u8>>>,
    ) -> anyhow::Result<u64> {
        for _ in 0..TRIES {
            let (had, expect) = match self.store.get(name)? {
                None => (None, Expect::Absent),
                Some((bytes, version)) => (Some(bytes), Expect::Version(version)),
            };
            let Some(next) = change(had)? else {
                return Ok(0);
            };
            match self.store.put(name, &next, &expect) {
                Ok(version) => return Ok(version_of(&version)),
                Err(err) if is_conflict(&err) => {
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(err) => return Err(err),
            }
        }
        bail!(
            "{} kept changing as it was written: try again",
            link_format::clean(name)
        )
    }
}

/// Whether `held` lapsed by `now`, not renewed for `lapse_ms`.
fn lapsed(held: &Held, now_ms: u64, lapse_ms: u64) -> bool {
    super::lapsed(held.renewed_ms, now_ms, lapse_ms)
}
