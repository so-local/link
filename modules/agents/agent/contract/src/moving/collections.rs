//! An agent's collections but Home in a move's stream: each read through
//! the store of the place the sender keeps it at, and written through the
//! receiver's. Every end of a move does both here (`lnk agent state-send` and
//! `receive`, a cloud's `agent-send` and `agent-receive`), so what's kept
//! with one driver arrives whole at another: `file` is a place like any
//! other, its store [`link_store_contract::store::file::Folder`].
//!
//! In the stream:
//!
//! - `files/instructions/<path>`, `files/skills/<path>`: each file, as
//!   the agent sees it ([`crate::folder::SEEN`]);
//! - `files/chats/<folder>/`: each conversation, as
//!   [`link_store_contract::store::folder`] makes its files;
//! - `schedule/<zone>/<id>.json`: each scheduled job in its place
//!   (`waiting`, `firing`, `ending`), the contract's [`Job`], in a switch
//!   alone, while the agent is paused.
//!
//! Each event and job is written in one form ([`canonical`]), so two
//! builds of `lnk` send the same bytes for the same conversation.

use link_format::WithinExt;
use std::collections::BTreeMap;

use anyhow::ensure;
use serde::{Serialize, Serializer};
use serde_json::Value;

use link_store_contract::storage::Collection;
use link_store_contract::store::folder::{Conversation, folder_of};
use link_store_contract::store::{Agent, Appended, Job, Place, Store, Written};

/// Where the stream holds what the agent sees.
pub const FILES: &str = "files";

/// Where the stream holds its scheduled jobs.
pub const SCHEDULE: &str = "schedule";

/// The places a job may be in, as the stream names them.
pub const PLACES: [Place; 4] = [Place::Waiting, Place::Firing, Place::Ending, Place::Ended];

/// The collections carried here: every one but Home, which is carried as
/// the files it is; memory's engine index travels separately.
pub const CARRIED: [Collection; 5] = [
    Collection::Instructions,
    Collection::Skills,
    Collection::Conversations,
    Collection::Schedule,
    Collection::Memory,
];

/// An agent's collections at one end of a move: for each carried
/// ([`CARRIED`]), the store of the place it's kept at there.
pub struct Kept<'a> {
    pub agent: Agent,
    stores: BTreeMap<Collection, &'a dyn Store>,
}

impl<'a> Kept<'a> {
    /// Agent `agent`'s, each collection's store given by `store_of`.
    pub fn new(agent: Agent, mut store_of: impl FnMut(Collection) -> &'a dyn Store) -> Self {
        let stores = CARRIED.into_iter().map(|c| (c, store_of(c))).collect();
        Self { agent, stores }
    }

    fn store(&self, collection: Collection) -> &'a dyn Store {
        self.stores[&collection]
    }
}

/// A place's name in the stream (`waiting`).
fn place_name(place: Place) -> &'static str {
    match place {
        Place::Waiting => "waiting",
        Place::Firing => "firing",
        Place::Ending => "ending",
        Place::Ended => "ended",
    }
}

/// The place `name` names in the stream.
fn place_named(name: &str) -> Option<Place> {
    PLACES.into_iter().find(|p| place_name(*p) == name)
}

/// The stream's path of job `id` in `place`: an error for an ID that
/// isn't one plain name.
pub fn job_path(place: Place, id: &str) -> anyhow::Result<String> {
    ensure!(
        !id.is_empty()
            && id.len() <= link_store_contract::store::JOB_ID_MAX
            && id != "."
            && id != ".."
            && !id.contains(['/', '\0']),
        "{:?} isn't a job's ID a move carries",
        link_format::clean(id)
    );
    Ok(format!("{SCHEDULE}/{}/{id}.json", place_name(place)))
}

/// The collection carried at a stream path, apart from Home.
pub fn at_path(path: &str) -> Option<Collection> {
    if let Some(path) = path.strip_prefix("files/") {
        return crate::folder::seen_collection(path);
    }
    if path.starts_with("memory/") {
        return Some(Collection::Memory);
    }
    if path.starts_with("schedule/") {
        return Some(Collection::Schedule);
    }
    None
}

/// The collection's file path in the move stream.
fn stream_path(collection: Collection, path: &str) -> String {
    match collection {
        Collection::Memory => path.to_string(),
        _ => format!("{FILES}/{path}"),
    }
}

/// The engine's SQLite backup, carried by its own index transfer.
pub fn is_index(path: &str) -> bool {
    let parts: Vec<_> = path.split('/').collect();
    parts.len() == 3 && parts[0] == "memory" && parts[2] == "index.sqlite"
}

/// Each entry of the stream `kept` holds, in order, its path and bytes,
/// handed to `entry` one at a time: its instructions, skills and
/// conversations, and in a switch (`with_jobs`) its jobs.
pub fn read(
    kept: &Kept,
    with_jobs: bool,
    mut entry: impl FnMut(&str, &[u8]) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let agent = &kept.agent;
    let mut old_notes = None;
    let mut notes = None;
    for collection in [
        Collection::Instructions,
        Collection::Skills,
        Collection::Memory,
    ] {
        let store = kept.store(collection);
        let folder = format!("{}/", collection.name());
        let paths = match collection {
            Collection::Memory => vec![link_harness_contract::harness::layout::NOTES.to_string()],
            _ => store.file_list(agent, &folder)?,
        };
        for path in paths {
            if let Some((bytes, _)) = store.file_read(agent, &path)? {
                if path == "instructions/notes.md" {
                    old_notes = Some(bytes);
                    continue;
                }
                if path == link_harness_contract::harness::layout::NOTES {
                    notes = Some(bytes);
                    continue;
                }
                entry(&stream_path(collection, &path), &bytes)?;
            }
        }
    }
    if let Some(mut notes) = notes.or_else(|| old_notes.take()) {
        if let Some(old) = old_notes
            && old != notes
        {
            notes.push(b'\n');
            notes.extend(old);
        }
        entry(link_harness_contract::harness::layout::NOTES, &notes)?;
    }
    let store = kept.store(Collection::Conversations);
    // Each with events, and each a list narrowed, events or none (one
    // removed keeps its narrowing): every tool there would widen it.
    let mut narrowed = store.tools_list(agent)?;
    let mut threads: Vec<String> = store
        .threads(agent)?
        .into_iter()
        .map(|k| k.thread)
        .collect();
    threads.extend(
        narrowed
            .keys()
            .filter(|t| !threads.contains(t))
            .cloned()
            .collect::<Vec<_>>(),
    );
    threads.sort();
    for thread in threads {
        let conversation = Conversation {
            events: store
                .read(agent, &thread, 1)?
                .iter()
                .map(canonical)
                .collect::<Result<_, _>>()?,
            tools: narrowed.remove(&thread),
            thread,
        };
        let folder = folder_of(&conversation.thread);
        for (name, bytes) in conversation.files()? {
            entry(
                &format!(
                    "{FILES}/{}/{folder}/{name}",
                    Collection::Conversations.directory()
                ),
                &bytes,
            )?;
        }
    }
    if with_jobs {
        let store = kept.store(Collection::Schedule);
        let (day, sent) = store.job_day(agent)?;
        entry("schedule/day.json", canonical(&(day, sent))?.as_bytes())?;
        for place in PLACES {
            let mut jobs = store.job_list(agent, place)?;
            jobs.sort_by(|a, b| a.id.cmp(&b.id));
            for job in jobs {
                entry(&job_path(place, &job.id)?, canonical(&job)?.as_bytes())?;
            }
        }
    }
    Ok(())
}

/// `value` as the stream writes it, whatever the build: compact JSON,
/// each object's keys in order. `serde_json` keeps a parsed object's keys
/// as they came when any package built with it turns on `preserve_order`,
/// and sorts them otherwise: written as parsed, the same event would hash
/// differently on two machines, and a move would send it again.
fn canonical(value: &impl Serialize) -> anyhow::Result<String> {
    struct Sorted<'a>(&'a Value);
    impl Serialize for Sorted<'_> {
        fn serialize<S: Serializer>(&self, to: S) -> Result<S::Ok, S::Error> {
            match self.0 {
                Value::Array(items) => to.collect_seq(items.iter().map(Sorted)),
                Value::Object(object) => {
                    let mut keys: Vec<_> = object.iter().collect();
                    keys.sort_unstable_by_key(|(key, _)| *key);
                    to.collect_map(keys.into_iter().map(|(key, value)| (key, Sorted(value))))
                }
                scalar => scalar.serialize(to),
            }
        }
    }
    Ok(serde_json::to_string(&Sorted(&serde_json::to_value(
        value,
    )?))?)
}

/// What a receiver has staged of a stream: each file by its path in it.
pub trait Staged {
    /// The paths staged under `prefix` (`files/skills/`), in any order.
    fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>>;

    /// What `path` holds.
    fn read(&self, path: &str) -> anyhow::Result<Vec<u8>>;
}

impl Staged for BTreeMap<String, Vec<u8>> {
    fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        Ok(self
            .range(prefix.to_string()..)
            .take_while(|(path, _)| path.starts_with(prefix))
            .map(|(path, _)| path.clone())
            .collect())
    }

    fn read(&self, path: &str) -> anyhow::Result<Vec<u8>> {
        self.get(path)
            .cloned()
            .within(format!("{} isn't staged", link_format::clean(path)))
    }
}

/// Validates the carried collections before a receiver changes any of
/// them or its settings. Home and the engine index are checked by their
/// streaming receiver.
pub fn check(staged: &dyn Staged) -> anyhow::Result<()> {
    for prefix in ["files/instructions/", "files/skills/"] {
        for path in staged.list(prefix)? {
            let file = path
                .strip_prefix("files/")
                .within("a collection path has no files prefix")?;
            link_store_contract::store::check_file_path(file, false)?;
            link_store_contract::store::check_file_size(file, staged.read(&path)?.len())?;
        }
    }
    if staged
        .list("memory/")?
        .iter()
        .any(|p| p == link_harness_contract::harness::layout::NOTES)
    {
        let notes = link_harness_contract::harness::layout::NOTES;
        link_store_contract::store::check_file_size(notes, staged.read(notes)?.len())?;
    }
    conversations(staged)?;
    jobs(staged)?;
    day(staged)?;
    Ok(())
}

/// Makes each collection of `kept` hold what `staged` does, a switch's
/// whole stream: what it holds that isn't staged removed, each file
/// written, each conversation's events past those it has added, each
/// conversation narrowed as the stream's is, each job kept in its place.
pub fn put(kept: &Kept, staged: &dyn Staged) -> anyhow::Result<()> {
    check(staged)?;
    let agent = &kept.agent;
    for collection in [
        Collection::Instructions,
        Collection::Skills,
        Collection::Memory,
    ] {
        put_files(kept.store(collection), agent, collection, staged)?;
    }
    put_conversations(kept.store(Collection::Conversations), agent, staged)?;
    put_jobs(kept.store(Collection::Schedule), agent, staged)
}

fn put_files(
    store: &dyn Store,
    agent: &Agent,
    collection: Collection,
    staged: &dyn Staged,
) -> anyhow::Result<()> {
    let prefix = format!("{FILES}/{}/", collection.name());
    let paths = match collection {
        Collection::Memory => staged
            .list("memory/")?
            .into_iter()
            .filter(|p| p == link_harness_contract::harness::layout::NOTES)
            .collect(),
        _ => staged.list(&prefix)?,
    };
    let mut wanted = BTreeMap::new();
    for path in paths {
        let bytes = staged.read(&path)?;
        let path = if collection == Collection::Memory {
            path
        } else {
            path[FILES.len() + 1..].to_string()
        };
        wanted.insert(path, bytes);
    }
    let paths = match collection {
        Collection::Memory => vec![link_harness_contract::harness::layout::NOTES.to_string()],
        _ => store.file_list(agent, &format!("{}/", collection.name()))?,
    };
    for path in paths {
        if wanted.contains_key(&path) {
            continue;
        }
        if let Some((_, version)) = store.file_read(agent, &path)? {
            store.file_remove(agent, &path, version)?;
        }
    }
    for (path, bytes) in &wanted {
        write_over(store, agent, path, bytes)?;
    }
    Ok(())
}

/// File `path` written with `bytes` over whatever version is there,
/// unless it holds them already; a write between the read and it is
/// tried again, twice.
fn write_over(store: &dyn Store, agent: &Agent, path: &str, bytes: &[u8]) -> anyhow::Result<()> {
    for _ in 0..3 {
        let version = match store.file_read(agent, path)? {
            Some((had, _)) if had == bytes => return Ok(()),
            Some((_, version)) => version,
            None => 0,
        };
        if let Written::Version(_) = store.file_write(agent, path, bytes, version)? {
            return Ok(());
        }
    }
    anyhow::bail!(
        "{} kept changing while it was written",
        link_format::clean(path)
    )
}

fn conversations(staged: &dyn Staged) -> anyhow::Result<Vec<Conversation>> {
    let prefix = format!("{FILES}/{}/", Collection::Conversations.directory());
    let mut folders: BTreeMap<String, BTreeMap<String, Vec<u8>>> = BTreeMap::new();
    for path in staged.list(&prefix)? {
        let Some((folder, name)) = path[prefix.len()..].split_once('/') else {
            continue;
        };
        if name.contains('/') {
            continue;
        }
        let bytes = staged.read(&path)?;
        folders
            .entry(folder.to_string())
            .or_default()
            .insert(name.to_string(), bytes);
    }
    let mut conversations = Vec::new();
    for (folder, files) in &folders {
        if let Some(conversation) = Conversation::from_files(folder, files)? {
            conversations.push(conversation);
        }
    }
    for conversation in &conversations {
        for (i, line) in conversation.events.iter().enumerate() {
            let event: Value = serde_json::from_str(line).within(format!(
                "an event of conversation {}",
                link_format::clean(&conversation.thread)
            ))?;
            ensure!(
                link_store_contract::store::seq_of(&event) == Some(i as u64 + 1),
                "conversation {} has an event out of order",
                link_format::clean(&conversation.thread)
            );
        }
    }
    Ok(conversations)
}

fn put_conversations(store: &dyn Store, agent: &Agent, staged: &dyn Staged) -> anyhow::Result<()> {
    let conversations = conversations(staged)?;
    for kept in store.threads(agent)? {
        if !conversations.iter().any(|c| c.thread == kept.thread) {
            store.remove(agent, &kept.thread)?;
        }
    }
    for conversation in conversations {
        let thread = &conversation.thread;
        let existing = store.read(agent, thread, 1)?;
        let last = store.last(agent, thread)?;
        ensure!(
            existing.len() <= conversation.events.len(),
            "conversation {} has events beyond the stream",
            link_format::clean(thread)
        );
        for (had, line) in existing.iter().zip(&conversation.events) {
            let sent: Value = serde_json::from_str(line)?;
            ensure!(
                *had == sent,
                "an event of conversation {} differs from the stream",
                link_format::clean(thread)
            );
        }
        for line in &conversation.events {
            let event: Value = serde_json::from_str(line).within(format!(
                "an event of conversation {}",
                link_format::clean(thread)
            ))?;
            let seq = link_store_contract::store::seq_of(&event).within(format!(
                "an event of conversation {} without its number",
                link_format::clean(thread)
            ))?;
            if seq <= last {
                continue;
            }
            match store.append(agent, thread, &event)? {
                Appended::Written => {}
                Appended::Taken(at) => anyhow::bail!(
                    "conversation {} holds event {at} already: its events there aren't the \
                     stream's",
                    link_format::clean(thread)
                ),
            }
        }
        if let Some(tools) = &conversation.tools {
            store.tools_narrow(agent, thread, tools)?;
        }
    }
    Ok(())
}

fn jobs(staged: &dyn Staged) -> anyhow::Result<BTreeMap<(Place, String), Job>> {
    let mut wanted: BTreeMap<(Place, String), Job> = BTreeMap::new();
    for path in staged.list(&format!("{SCHEDULE}/"))? {
        if path == "schedule/day.json" {
            continue;
        }
        let rest = &path[SCHEDULE.len() + 1..];
        let (place, name) = rest
            .split_once('/')
            .and_then(|(place, name)| Some((place_named(place)?, name.strip_suffix(".json")?)))
            .within(format!(
                "{} isn't a job in the stream",
                link_format::clean(&path)
            ))?;
        let job: Job = serde_json::from_slice(&staged.read(&path)?)
            .within(format!("reading {}", link_format::clean(&path)))?;
        ensure!(
            job.id == name && job_path(place, &job.id)? == path,
            "{} holds another job",
            link_format::clean(&path)
        );
        link_store_contract::store::check_job(&job)?;
        wanted.insert((place, job.id.clone()), job);
    }
    Ok(wanted)
}

fn day(staged: &dyn Staged) -> anyhow::Result<(u64, u32)> {
    if staged
        .list("schedule/")?
        .iter()
        .any(|p| p == "schedule/day.json")
    {
        Ok(serde_json::from_slice(&staged.read("schedule/day.json")?)?)
    } else {
        Ok((0, 0))
    }
}

fn put_jobs(store: &dyn Store, agent: &Agent, staged: &dyn Staged) -> anyhow::Result<()> {
    let wanted = jobs(staged)?;
    for place in PLACES {
        for job in store.job_list(agent, place)? {
            if !wanted.contains_key(&(place, job.id.clone())) {
                store.job_remove(agent, place, &job.id)?;
            }
        }
    }
    let (day, sent) = day(staged)?;
    store.job_set_day(agent, day, sent)?;
    for ((place, _), job) in &wanted {
        store.job_put(agent, *place, job)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_store_contract::store::file::Folder;

    fn agent() -> Agent {
        Agent {
            id: "aaaaaaaaaaaa".into(),
            owner: "me@box".into(),
        }
    }

    fn event(seq: u64, role: &str, text: &str) -> Value {
        serde_json::json!({"seq": seq, "role": role, "text": text, "at": "2026-10-14T10:00:00Z"})
    }

    fn job(id: &str, due: u64) -> Job {
        Job {
            id: id.into(),
            spec: serde_json::json!({"id": id, "kind": "schedule", "text": "hi"}),
            due,
        }
    }

    fn staged(kept: &Kept, with_jobs: bool) -> BTreeMap<String, Vec<u8>> {
        let mut staged = BTreeMap::new();
        read(kept, with_jobs, |path, bytes| {
            staged.insert(path.to_string(), bytes.to_vec());
            Ok(())
        })
        .unwrap();
        staged
    }

    /// A folder's every collection read into a stream and put into
    /// another folder holds there what it held here, and what the other
    /// held that the stream doesn't is gone.
    #[test]
    fn every_collection_arrives_whole_and_nothing_else_stays() {
        let dir = link_test_kit::TempDir::new("collections-whole");
        let here = Folder::at(&dir.join("here"), "aaaaaaaaaaaa").unwrap();
        let there = Folder::at(&dir.join("there"), "aaaaaaaaaaaa").unwrap();
        let (here, there): (&dyn Store, &dyn Store) = (&here, &there);
        let a = agent();
        let from = Kept::new(a.clone(), |_| here);
        let to = Kept::new(a.clone(), |_| there);

        here.file_write(&a, "instructions/instruction.md", b"Be kind.", 0)
            .unwrap();
        here.file_write(&a, "skills/web/SKILL.md", b"# Web", 0)
            .unwrap();
        here.append(&a, "telegram:42", &event(1, "user", "Hi"))
            .unwrap();
        here.append(&a, "telegram:42", &event(2, "assistant", "Hello."))
            .unwrap();
        here.tools_narrow(&a, "task:1", &["memory__search".into()])
            .unwrap();
        here.job_put(&a, Place::Waiting, &job("j1", 100)).unwrap();
        here.job_put(&a, Place::Firing, &job("j2", 50)).unwrap();
        // Removed, its narrowing kept: it moves narrow.
        here.append(&a, "task:2", &event(1, "user", "go")).unwrap();
        here.tools_narrow(&a, "task:2", &[]).unwrap();
        here.remove(&a, "task:2").unwrap();

        // What the other end held before: one stale of each.
        there
            .file_write(&a, "instructions/old.md", b"gone", 0)
            .unwrap();
        there
            .append(&a, "slack:D1", &event(1, "user", "old"))
            .unwrap();
        there.job_put(&a, Place::Waiting, &job("j9", 1)).unwrap();
        // And the first of telegram:42, as a move back would find it.
        there
            .append(&a, "telegram:42", &event(1, "user", "Hi"))
            .unwrap();

        let stream = staged(&from, true);
        put(&to, &stream).unwrap();
        assert_eq!(staged(&to, true), stream);
        assert_eq!(
            there.file_list(&a, "instructions/").unwrap(),
            ["instructions/instruction.md"]
        );
        assert_eq!(there.read(&a, "telegram:42", 1).unwrap().len(), 2);
        assert_eq!(
            there.tools_read(&a, "task:1").unwrap(),
            Some(vec!["memory__search".to_string()])
        );
        assert_eq!(there.tools_read(&a, "task:2").unwrap(), Some(Vec::new()));
        assert_eq!(there.last(&a, "task:2").unwrap(), 0);
        assert_eq!(
            there.job_get(&a, Place::Firing, "j2").unwrap(),
            Some(job("j2", 50))
        );
        assert_eq!(there.job_get(&a, Place::Waiting, "j9").unwrap(), None);
        // Put again, nothing changes.
        put(&to, &stream).unwrap();
        assert_eq!(staged(&to, true), stream);
    }

    /// An event and a job are sent in one form, their keys in order at
    /// every depth, whatever order they're kept in and whichever features
    /// `serde_json` was built with: two builds of `lnk` hash them the same.
    #[test]
    fn an_event_and_a_job_are_sent_as_the_same_bytes_by_every_build() {
        let dir = link_test_kit::TempDir::new("collections-bytes");
        let here = Folder::at(&dir.join("here"), "aaaaaaaaaaaa").unwrap();
        let here: &dyn Store = &here;
        let a = agent();
        let kept: Value = serde_json::from_str(
            r#"{"seq":1,"text":"Hi","role":"user","said":{"z":1,"a":[{"y":2,"b":"é"}]}}"#,
        )
        .unwrap();
        here.append(&a, "telegram:42", &kept).unwrap();
        here.job_put(&a, Place::Waiting, &job("j1", 100)).unwrap();
        let stream = staged(&Kept::new(a, |_| here), true);
        assert_eq!(
            String::from_utf8_lossy(&stream["files/chats/telegram-42/events.jsonl"]),
            "{\"role\":\"user\",\"said\":{\"a\":[{\"b\":\"é\",\"y\":2}],\"z\":1},\"seq\":1,\"text\":\"Hi\"}\n"
        );
        assert_eq!(
            String::from_utf8_lossy(&stream["schedule/waiting/j1.json"]),
            r#"{"due":100,"id":"j1","spec":{"id":"j1","kind":"schedule","text":"hi"}}"#
        );
    }

    /// A copy carries no jobs: the agent still runs, and they're its
    /// scheduler's until it's paused.
    #[test]
    fn jobs_are_carried_in_a_switch_alone() {
        let dir = link_test_kit::TempDir::new("collections-jobs");
        let here = Folder::at(&dir.join("here"), "aaaaaaaaaaaa").unwrap();
        let here: &dyn Store = &here;
        let a = agent();
        here.job_put(&a, Place::Waiting, &job("j1", 100)).unwrap();
        let from = Kept::new(a, |_| here);
        assert!(
            staged(&from, false)
                .keys()
                .all(|p| !p.starts_with("schedule/"))
        );
        assert_eq!(
            staged(&from, true).keys().collect::<Vec<_>>(),
            ["schedule/day.json", "schedule/waiting/j1.json"]
        );
    }

    /// A stream whose job's file names another job, or a place that isn't
    /// one, is refused.
    #[test]
    fn a_job_the_stream_misnames_is_refused() {
        let dir = link_test_kit::TempDir::new("collections-misnamed");
        let there = Folder::at(&dir.join("there"), "aaaaaaaaaaaa").unwrap();
        let to = Kept::new(agent(), |_| &there as &dyn Store);
        let bytes = serde_json::to_vec(&job("j1", 1)).unwrap();
        for path in ["schedule/waiting/j2.json", "schedule/later/j1.json"] {
            let stream = BTreeMap::from([(path.to_string(), bytes.clone())]);
            assert!(put(&to, &stream).is_err(), "{path}");
        }
    }
}
