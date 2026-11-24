//! The `file` place's driver (`lnk-file-store`): an agent's collections in its
//! folder on this machine, as a person reads and edits them. Run by the
//! storage bridge outside every sandbox, it reaches the folder only
//! through [`super::under`]: a link in it is never followed.
//!
//! In the folder (`LNK_STORE_DIR`, the agent's own, `agent/`,
//! [`crate::folder`]):
//!
//! ```text
//! <collection>/<path>                       a file by its path, as written (instructions/instruction.md)
//! chats/<folder>/events.jsonl       a conversation's events, a line each, only added to
//! chats/<folder>/conversation.md    its copy for people, up to the agent's last answer
//! chats/<folder>/tools.json         the tools it was narrowed to
//! chats/<folder>/turn.json          its turn's lease
//! schedule/jobs/<id>.json                   a job waiting for its time
//! schedule/firing/<id>.json                 a job being fired
//! schedule/firing/<id>.ending               a job whose firing ends
//! schedule/day.json                         the day's count of its messages
//! schedule/mark                             what changes with what waits
//! ```
//!
//! A conversation's folder is its thread's name, each `:` a `-`
//! ([`super::folder`]). One kept `compact` (frames in `events.zst`,
//! [`super::compact`]) goes on compact. File revisions are reserved from
//! a persistent counter under `.lnk/revisions/`, apart from the collections.
//! Their records detect a person's edits too; a revision never returns
//! when the same contents are written or a removed file is recreated.
//! A job's file is the contract's [`Job`], as JSON.
//! Memory's notes live at `memory/notes.md`; an engine keeps its own
//! state at `memory/<engine>/`, with its index's copy alongside it.

use std::io::{Read, Seek, Write};

use anyhow::{Context, bail};
use serde_json::Value;

use super::copy::{self, Said};
use super::folder::{self, EVENTS, MARKDOWN, TOOLS};
use super::under::{Dir, Kind, lock_fd};
use super::{
    EVENT_MAX, FILE_MAX, FILES_MAX, JOB_MAX, Job, Kept, Lease, Place, Reply, Request, Stored, Took,
    check_file_path, check_tools, file_data, seq_of,
};

#[cfg(feature = "compact")]
mod compact;
mod jobs;
mod logged;
mod revisions;
#[cfg(test)]
mod tests;
#[cfg(not(feature = "compact"))]
mod uncompact;

use jobs::mark_of;
use logged::*;
#[cfg(not(feature = "compact"))]
use uncompact as compact;

/// A conversation's turn's lease.
pub const TURN: &str = "turn.json";

/// The agent's scheduled jobs' folder, in its own.
pub const SCHEDULE: &str = "schedule";

/// The day's count, in [`SCHEDULE`].
const DAY: &str = "day.json";

/// The mark, in [`SCHEDULE`]: a number, moved on with each change to
/// what waits.
const MARK: &str = "mark";

/// The day's count of the agent's messages, as [`DAY`] holds it.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Day {
    day: u64,
    sent: u32,
}

/// The most a lease's file holds.
const LEASE_MAX: u64 = 4 << 10;

/// How many events before an answer the copy for people is brought up
/// to date from; past them it's made anew from every event.
const COPY_BACK: u64 = 64;

/// Driver-owned data paths relative to its root, kept with the raw
/// collections in a backup. Raw collection transfer into another backend
/// does not transfer that backend's opaque version identity.
pub fn state_paths() -> [std::path::PathBuf; 1] {
    [link_files::layout::Root::Hidden
        .default_in(std::path::Path::new(""))
        .join(revisions::FOLDER)]
}

/// The agent's folder on this machine.
pub struct Folder {
    root: Dir,
    /// The agent whose folder it is: any other's call is refused.
    agent: String,
    schedule: String,
}

/// The folder conversation `thread` is kept in: an error for a thread
/// whose folder would be anything but one name.
fn conversation(thread: &str) -> anyhow::Result<String> {
    let name = folder::folder_of(thread);
    super::under::check_part(&name).with_context(|| {
        format!(
            "{:?} isn't a conversation's name",
            link_format::clean(thread)
        )
    })?;
    anyhow::ensure!(
        !name.starts_with('.'),
        "{:?} isn't a conversation's name",
        link_format::clean(thread)
    );
    Ok(name)
}

/// What a conversation's lease file holds.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Held {
    holder: String,
    thread: String,
    started: u64,
    renewed: u64,
    /// When it was renewed to the millisecond: a lease's lapse is
    /// judged by it, never by `renewed`'s whole second (up to a second
    /// early).
    renewed_ms: u64,
}

impl super::Store for Folder {
    fn name(&self) -> &str {
        "file"
    }

    fn call(&self, request: &Request) -> anyhow::Result<Reply> {
        self.answer(request)
    }
}

impl Folder {
    /// Agent `agent`'s (its ID) folder at `path`, made (700) if it isn't
    /// there.
    pub fn at(path: &std::path::Path, agent: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(path.is_absolute(), "{} isn't absolute", path.display());
        link_files::create_private_dir(path)
            .with_context(|| format!("making {}", path.display()))?;
        let root = Dir::root(path)?.with_context(|| format!("{} isn't there", path.display()))?;
        Ok(Self {
            root,
            agent: agent.to_string(),
            schedule: SCHEDULE.to_string(),
        })
    }

    /// One call, answered: an error to refuse it.
    pub fn answer(&self, request: &Request) -> anyhow::Result<Reply> {
        anyhow::ensure!(
            request.agent().id == self.agent,
            "this is agent {}'s folder: agent {}'s call is refused",
            link_format::clean(&self.agent),
            link_format::clean(&request.agent().id)
        );
        Ok(match request {
            Request::Append { thread, event, .. } => self.append(thread, event)?,
            Request::Read { thread, first, .. } => Reply::Events {
                events: self.read(thread, *first)?,
            },
            Request::Last { thread, .. } => Reply::Last {
                last: match self.conv(thread, false)? {
                    Some(conv) => last_of(&conv)?,
                    None => 0,
                },
            },
            Request::Threads { .. } => Reply::Threads {
                threads: self.threads()?,
            },
            Request::Remove { thread, .. } => {
                let name = conversation(thread)?;
                if let Some(conversations) = self
                    .root
                    .sub(link_harness_base::layout::CONVERSATIONS, false)?
                    && let Some(conv) = conversations.sub(&name, false)?
                {
                    // Its events, copy and lease; the tools it was
                    // narrowed to stay, with its name, so it's never made
                    // again with more.
                    let narrowed = conv.kind(TOOLS)? == Some(Kind::File);
                    for file in conv.names()? {
                        if file != TOOLS && conv.kind(&file)? != Some(Kind::Dir) {
                            conv.remove(&file)?;
                        }
                    }
                    match narrowed {
                        true => conv.write(MARKDOWN, copy::header(thread).as_bytes())?,
                        false => {
                            conversations.remove_dir(&name)?;
                        }
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
            Request::Renew { thread, holder, .. } => {
                let Some(conv) = self.conv(thread, false)? else {
                    return Ok(Reply::Lost);
                };
                let _locked = conv.lock(libc::LOCK_EX)?;
                match lease(&conv)? {
                    Some(mut held) if held.holder == *holder => {
                        let now = super::now_ms();
                        held.renewed = now / 1000;
                        held.renewed_ms = now;
                        conv.write(TURN, &serde_json::to_vec(&held)?)?;
                        Reply::Done
                    }
                    _ => Reply::Lost,
                }
            }
            Request::End { thread, holder, .. } => {
                if let Some(conv) = self.conv(thread, false)? {
                    let _locked = conv.lock(libc::LOCK_EX)?;
                    if lease(&conv)?.is_some_and(|held| held.holder == *holder) {
                        conv.remove(TURN)?;
                    }
                }
                Reply::Done
            }
            Request::Leases { lapse_ms, .. } => Reply::Leases {
                leases: self.leases(*lapse_ms)?,
            },
            Request::ToolsRead { thread, .. } => Reply::Tools {
                tools: match self.conv(thread, false)? {
                    Some(conv) => conv
                        .read(TOOLS, folder::TOOLS_FILE_MAX)?
                        .map(|bytes| folder::read_tools(&bytes))
                        .transpose()?,
                    None => None,
                },
            },
            Request::ToolsList { .. } => Reply::Narrowed {
                narrowed: self.narrowed()?,
            },
            Request::ToolsNarrow { thread, tools, .. } => {
                check_tools(tools)?;
                let conv = self.conv(thread, true)?.context("no conversation")?;
                let _locked = conv.lock(libc::LOCK_EX)?;
                let narrowed: Vec<String> = match conv.read(TOOLS, folder::TOOLS_FILE_MAX)? {
                    Some(bytes) => {
                        let had = folder::read_tools(&bytes)?;
                        tools.iter().filter(|t| had.contains(t)).cloned().collect()
                    }
                    None => tools.clone(),
                };
                conv.write(TOOLS, &folder::tools_file(&narrowed)?)?;
                // Narrowed before its first event, its copy's first line
                // names its thread: its folder's name can't.
                if conv.kind(MARKDOWN)?.is_none() {
                    conv.write(MARKDOWN, copy::header(thread).as_bytes())?;
                }
                Reply::Tools {
                    tools: Some(narrowed),
                }
            }
            Request::FileRead { path, .. } => {
                check_file_path(path, false)?;
                let revisions = self.revisions()?;
                let _locked = revisions.lock(libc::LOCK_EX)?;
                let file = match self.found(path)? {
                    Some((dir, name)) => revisions::read(&dir, &name, super::file_max(path))?,
                    None => None,
                };
                let version = revisions::current(&revisions, path, file.as_ref())?;
                Reply::File {
                    file: file.map(|file| {
                        use base64::Engine as _;
                        Stored {
                            version,
                            data: base64::engine::general_purpose::STANDARD.encode(file.bytes),
                        }
                    }),
                }
            }
            Request::FileList { folder, .. } => {
                check_file_path(folder, true)?;
                let folder = folder.trim_end_matches('/');
                let mut files = Vec::new();
                if let Some(dir) = self.root.walk(folder, false)? {
                    listed(&dir, folder, &mut files)?;
                }
                files.sort();
                Reply::Files { files }
            }
            Request::FileWrite {
                path,
                data,
                version: expected,
                ..
            } => {
                check_file_path(path, false)?;
                let data = file_data(path, data)?;
                let revisions = self.revisions()?;
                let _revision_lock = revisions.lock(libc::LOCK_EX)?;
                let (dir, name) = self.place_of(path, true)?;
                let _locked = dir.lock(libc::LOCK_EX)?;
                let now = revisions::current(
                    &revisions,
                    path,
                    revisions::read(&dir, &name, super::file_max(path))?.as_ref(),
                )?;
                if now != *expected {
                    return Ok(Reply::Stale { version: now });
                }
                let version = revisions::reserve(&revisions)?;
                dir.write(&name, &data)?;
                revisions::remember(&revisions, path, &dir, &name, version)?;
                Reply::Wrote { version }
            }
            Request::FileRemove {
                path,
                version: expected,
                ..
            } => {
                check_file_path(path, false)?;
                let revisions = self.revisions()?;
                let _revision_lock = revisions.lock(libc::LOCK_EX)?;
                let Some((dir, name)) = self.found(path)? else {
                    return Ok(Reply::Found { found: false });
                };
                let _locked = dir.lock(libc::LOCK_EX)?;
                let now = revisions::current(
                    &revisions,
                    path,
                    revisions::read(&dir, &name, super::file_max(path))?.as_ref(),
                )?;
                let found = now != 0 && now == *expected && dir.remove(&name)?;
                Reply::Found { found }
            }
            Request::FileAppend { path, data, .. } => {
                check_file_path(path, false)?;
                let data = file_data(path, data)?;
                let revisions = self.revisions()?;
                let _revision_lock = revisions.lock(libc::LOCK_EX)?;
                let (dir, name) = self.place_of(path, true)?;
                let _locked = dir.lock(libc::LOCK_EX)?;
                let image = revisions::read(&dir, &name, super::file_max(path))?;
                revisions::current(&revisions, path, image.as_ref())?;
                let mut body = image.map(|image| image.bytes).unwrap_or_default();
                anyhow::ensure!(
                    (body.len() + data.len()) as u64 <= super::file_max(path),
                    "{} would be larger than {FILE_MAX} bytes",
                    link_format::clean(path)
                );
                body.extend_from_slice(&data);
                let version = revisions::reserve(&revisions)?;
                dir.write(&name, &body)?;
                revisions::remember(&revisions, path, &dir, &name, version)?;
                Reply::Wrote { version }
            }
            Request::JobPut { place, job, .. } => {
                self.job_put(*place, job)?;
                Reply::Done
            }
            Request::JobGet { place, id, .. } => Reply::Job {
                job: self.job_get(*place, id)?,
            },
            Request::JobList { place, .. } => Reply::Jobs {
                jobs: self.job_list(*place)?,
            },
            Request::JobCount { place, .. } => Reply::Count {
                count: self.job_list(*place)?.len() as u64,
            },
            Request::JobShift { id, from, to, .. } => Reply::Found {
                found: self.job_shift(id, *from, *to)?,
            },
            Request::JobRemove { place, id, .. } => Reply::Found {
                found: self.job_remove(*place, id)?,
            },
            Request::JobNext { .. } => Reply::Next {
                next: self.job_list(Place::Waiting)?.iter().map(|j| j.due).min(),
            },
            Request::JobDay { .. } => {
                let day: Day = match self.root.walk(&self.schedule, false)? {
                    Some(schedule) => schedule
                        .read(DAY, LEASE_MAX)?
                        .map(|bytes| serde_json::from_slice(&bytes))
                        .transpose()
                        .context("reading the day's count")?
                        .unwrap_or_default(),
                    None => Day::default(),
                };
                Reply::Day {
                    day: day.day,
                    sent: day.sent,
                }
            }
            Request::JobSetDay { day, sent, .. } => {
                let schedule = self.schedule()?;
                let day = Day {
                    day: *day,
                    sent: *sent,
                };
                schedule.write(DAY, &serde_json::to_vec(&day)?)?;
                Reply::Done
            }
            Request::JobMark { .. } => Reply::Mark {
                mark: match self.root.walk(&self.schedule, false)? {
                    Some(schedule) => mark_of(&schedule)?,
                    None => 0,
                },
            },
        })
    }

    /// File `path`'s folder and name: the folder made when `make`.
    fn place_of(&self, path: &str, make: bool) -> anyhow::Result<(Dir, String)> {
        let (folder, name) = path.rsplit_once('/').context("a file needs a folder")?;
        let dir = self
            .root
            .walk(folder, make)?
            .with_context(|| format!("{} isn't there", link_format::clean(folder)))?;
        Ok((dir, name.to_string()))
    }

    /// File `path`'s folder and name, none when its folder isn't there; an
    /// error for a link on the way.
    fn found(&self, path: &str) -> anyhow::Result<Option<(Dir, String)>> {
        let (folder, name) = path.rsplit_once('/').context("a file needs a folder")?;
        Ok(self
            .root
            .walk(folder, false)?
            .map(|dir| (dir, name.to_string())))
    }

    /// Version state stays outside every collection a caller can list.
    fn revisions(&self) -> anyhow::Result<Dir> {
        let mut at = self.root.walk("", false)?.context("driver root")?;
        for component in state_paths()[0].components() {
            let std::path::Component::Normal(name) = component else {
                bail!("driver state must be a child-relative path");
            };
            let next = at
                .sub(name.to_str().context("driver state path is text")?, true)?
                .context("driver revision state")?;
            at.sync_all()?;
            at = next;
        }
        Ok(at)
    }

    /// Conversation `thread`'s folder: made when `make`, else none when
    /// it isn't there.
    fn conv(&self, thread: &str, make: bool) -> anyhow::Result<Option<Dir>> {
        let name = conversation(thread)?;
        let Some(conversations) = self
            .root
            .sub(link_harness_base::layout::CONVERSATIONS, make)?
        else {
            return Ok(None);
        };
        Ok(conversations.sub(&name, make)?)
    }

    fn append(&self, thread: &str, event: &Value) -> anyhow::Result<Reply> {
        let seq = seq_of(event).context("an event without its number")?;
        let line = line_of(event)?;
        let conv = self.conv(thread, true)?.context("no conversation")?;
        if compacted(&conv)? {
            return compact::append(&conv, seq, &line);
        }
        let mut file = conv.open(EVENTS, libc::O_RDWR | libc::O_CREAT, 0o600)?;
        // One writer at a time, of any process: the last number is read
        // and the next written while it's held.
        lock_fd(std::os::fd::AsRawFd::as_raw_fd(&file), libc::LOCK_EX)?;
        let last = last_seq(&mut file, true)?.unwrap_or(0);
        if last.checked_add(1) != Some(seq) {
            return Ok(Reply::Taken { last });
        }
        file.seek(std::io::SeekFrom::End(0))?;
        file.write_all(&line)?;
        drop(file);
        self.copy(&conv, thread, event)?;
        Ok(Reply::Done)
    }

    fn read(&self, thread: &str, first: u64) -> anyhow::Result<Vec<Value>> {
        let Some(conv) = self.conv(thread, false)? else {
            return Ok(Vec::new());
        };
        read_conv(&conv, first)
    }

    fn threads(&self) -> anyhow::Result<Vec<Kept>> {
        let Some(conversations) = self
            .root
            .sub(link_harness_base::layout::CONVERSATIONS, false)?
        else {
            return Ok(Vec::new());
        };
        let mut kept = Vec::new();
        for name in conversations.names()? {
            if name.starts_with('.') || conversations.kind(&name)? != Some(Kind::Dir) {
                continue;
            }
            let Some(conv) = conversations.sub(&name, false)? else {
                continue;
            };
            let last = last_of(&conv)?;
            if last == 0 {
                continue;
            }
            kept.push(Kept {
                thread: thread_in(&conv, &name),
                last,
            });
        }
        kept.sort();
        Ok(kept)
    }

    /// Each conversation a list narrowed, events or none.
    fn narrowed(&self) -> anyhow::Result<std::collections::BTreeMap<String, Vec<String>>> {
        let mut narrowed = std::collections::BTreeMap::new();
        let Some(conversations) = self
            .root
            .sub(link_harness_base::layout::CONVERSATIONS, false)?
        else {
            return Ok(narrowed);
        };
        for name in conversations.names()? {
            if name.starts_with('.') || conversations.kind(&name)? != Some(Kind::Dir) {
                continue;
            }
            let Some(conv) = conversations.sub(&name, false)? else {
                continue;
            };
            if let Some(bytes) = conv.read(TOOLS, folder::TOOLS_FILE_MAX)? {
                narrowed.insert(thread_in(&conv, &name), folder::read_tools(&bytes)?);
            }
        }
        Ok(narrowed)
    }

    fn take(&self, thread: &str, holder: &str, lapse_ms: u64) -> anyhow::Result<Took> {
        let conv = self.conv(thread, true)?.context("no conversation")?;
        // One taker at a time, of any process.
        let _locked = conv.lock(libc::LOCK_EX)?;
        let now = super::now_ms();
        let took = match lease(&conv)? {
            None => Took::Took,
            Some(held) if !lapsed(&held, now, lapse_ms) => return Ok(Took::Held),
            Some(_) => Took::Lapsed,
        };
        let held = Held {
            holder: holder.to_string(),
            thread: thread.to_string(),
            started: now / 1000,
            renewed: now / 1000,
            renewed_ms: now,
        };
        conv.write(TURN, &serde_json::to_vec(&held)?)?;
        Ok(took)
    }

    fn leases(&self, lapse_ms: u64) -> anyhow::Result<Vec<Lease>> {
        let Some(conversations) = self
            .root
            .sub(link_harness_base::layout::CONVERSATIONS, false)?
        else {
            return Ok(Vec::new());
        };
        let now = super::now_ms();
        let mut leases = Vec::new();
        for name in conversations.names()? {
            let Ok(Some(conv)) = conversations.sub(&name, false) else {
                continue;
            };
            let Ok(Some(held)) = lease(&conv) else {
                continue;
            };
            // Only a thread whose folder this is: a lease naming another
            // is no one's.
            if folder::folder_of(&held.thread) != name {
                continue;
            }
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

    /// Brings the copy for people up to `event`, just written: the
    /// sections since the agent's last answer added to its end when the
    /// copy ends with that answer, else the copy made anew from every
    /// event. Only an answer adds to it.
    fn copy(&self, conv: &Dir, thread: &str, event: &Value) -> anyhow::Result<()> {
        let exists = conv.kind(MARKDOWN)? == Some(Kind::File);
        if exists && !Said::of(event).is_answer() {
            return Ok(());
        }
        let seq = seq_of(event).unwrap_or_default();
        let first = seq.saturating_sub(COPY_BACK).max(1);
        let mut before = read_conv(conv, first)?;
        before.retain(|e| seq_of(e).is_some_and(|s| s < seq));
        let whole = first == 1;
        let said: Vec<Said> = before.iter().map(Said::of).collect();
        let last = said.iter().rposition(Said::is_answer);
        let so_far = match last {
            Some(i) => copy::section(&said[i]),
            None if whole => Some(copy::header(thread)),
            None => None,
        };
        if exists
            && let Some(so_far) = so_far
            && ends_with(conv, MARKDOWN, so_far.as_bytes(), last.is_none())?
        {
            let from = last.map_or(0, |i| i + 1);
            let mut added = String::new();
            for one in said[from..].iter().chain([&Said::of(event)]) {
                added += &copy::section(one).unwrap_or_default();
            }
            let mut file = conv.open(MARKDOWN, libc::O_WRONLY | libc::O_APPEND, 0)?;
            file.write_all(added.as_bytes())?;
            return Ok(());
        }
        let every = match whole {
            true => [before, vec![event.clone()]].concat(),
            false => read_conv(conv, 1)?,
        };
        let said: Vec<Said> = every.iter().map(Said::of).collect();
        conv.write(MARKDOWN, copy::markdown(thread, &said).as_bytes())?;
        Ok(())
    }
}

/// Every file under `dir` (whose path is `at`), by its path, never a link,
/// a file written aside, or more than [`FILES_MAX`].
fn listed(dir: &Dir, at: &str, files: &mut Vec<String>) -> anyhow::Result<()> {
    for name in dir.names()? {
        if link_files::layout::is_temporary(&name) {
            continue;
        }
        let path = format!("{at}/{name}");
        match dir.kind(&name)? {
            Some(Kind::File) => {
                anyhow::ensure!(
                    files.len() < FILES_MAX,
                    "{} holds more than {FILES_MAX} files",
                    link_format::clean(at)
                );
                files.push(path);
            }
            Some(Kind::Dir) => {
                if let Some(sub) = dir.sub(&name, false)? {
                    listed(&sub, &path, files)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// The frames of a conversation kept compact ([`super::compact`]).
const FRAMES: &str = "events.zst";

/// Whether the conversation in `conv` is kept compact.
fn compacted(conv: &Dir) -> anyhow::Result<bool> {
    Ok(conv.kind(FRAMES)?.is_some())
}

/// The conversation in `conv`, numbered `first` and on.
fn read_conv(conv: &Dir, first: u64) -> anyhow::Result<Vec<Value>> {
    if compacted(conv)? {
        return compact::read(conv, first);
    }
    read_log(conv, first)
}

/// The number of the conversation's last event, 0 for none.
fn last_of(conv: &Dir) -> anyhow::Result<u64> {
    if compacted(conv)? {
        return compact::last(conv);
    }
    let last = match conv.open(EVENTS, libc::O_RDONLY, 0) {
        Ok(mut file) => {
            lock_fd(std::os::fd::AsRawFd::as_raw_fd(&file), libc::LOCK_SH)?;
            last_seq(&mut file, false)?
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err.into()),
    };
    Ok(last.unwrap_or(0))
}

/// The thread the conversation in `conv`, folder `name`, is of: its copy's
/// first line, or its compact frames' name, where that names this folder;
/// else the folder's name.
fn thread_in(conv: &Dir, name: &str) -> String {
    let from_copy = conv
        .open(MARKDOWN, libc::O_RDONLY, 0)
        .ok()
        .and_then(|file| {
            let mut head = Vec::new();
            file.take(8 << 10).read_to_end(&mut head).ok()?;
            let first = String::from_utf8_lossy(&head).lines().next()?.to_string();
            first.strip_prefix("# ").map(str::to_string)
        });
    from_copy
        .or_else(|| compact::thread(conv))
        .filter(|t| folder::folder_of(t) == name)
        .unwrap_or_else(|| name.to_string())
}

/// The lease in `conv`, if one is there.
fn lease(conv: &Dir) -> anyhow::Result<Option<Held>> {
    match conv.read(TURN, LEASE_MAX)? {
        None => Ok(None),
        // Unreadable, it's no one's lease: taken over as lapsed.
        Some(bytes) => Ok(Some(serde_json::from_slice(&bytes).unwrap_or(Held {
            holder: String::new(),
            thread: String::new(),
            started: 0,
            renewed: 0,
            renewed_ms: 0,
        }))),
    }
}

/// Whether `held` lapsed by `now`, not renewed for `lapse_ms`.
fn lapsed(held: &Held, now_ms: u64, lapse_ms: u64) -> bool {
    super::lapsed(held.renewed_ms, now_ms, lapse_ms)
}
