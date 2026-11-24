//! An append-only portable archive beside the existing native session engine.
//! Each admitted record is synced before forwarding or acknowledging it.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    io::Write,
    path::Path,
    sync::Mutex,
};

use agent_client_protocol::schema::v1::{ContentChunk, PromptRequest, SessionUpdate};
use anyhow::{Context as _, ensure};
use link_files::nofollow::Dir;
use link_harness_contract::{
    asp::forms::Session,
    harness::continuation::{Context, Prepared},
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Limits {
    record_max: u64,
}

static LIMITS: std::sync::LazyLock<Limits> = std::sync::LazyLock::new(|| {
    toml::from_str(include_str!("continuity.toml")).expect("checked portable capture limits")
});

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    adapter: String,
    #[serde(default)]
    agent: Option<link_harness_contract::store::Agent>,
    thread: String,
    change: Change,
}

link_files::specification!(Record, include_str!("continuity/record.schema.toml"));

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Change {
    Import {
        sessions: Vec<Session>,
        pending: bool,
    },
    Append {
        updates: Vec<SessionUpdate>,
        delivery: Option<String>,
    },
    Supplied {
        context: Prepared,
        memory: String,
    },
}

struct Thread {
    session: Session,
    pending: bool,
    memory: String,
    deliveries: BTreeSet<String>,
}

#[derive(Default)]
struct Kept {
    threads: BTreeMap<String, Thread>,
    sources: BTreeMap<(String, String, String), serde_json::Value>,
    next: u64,
}

pub(crate) struct Archive {
    _lock: std::fs::File,
    dir: Dir,
    context: Context,
    kept: Mutex<Result<Kept, String>>,
}

impl Archive {
    pub fn at(home: &Path, context: Context) -> anyhow::Result<Self> {
        let folder = link_harness_contract::harness::continuation::folder(home);
        let lock = link_files::lock_file(&folder.join("handoff.lock"))?;
        let parent = Dir::open_unlinked(&folder)?;
        let dir = parent.create_folder(OsStr::new("handoffs"), 0o700)?;
        parent.sync_all()?;
        let mut kept = Kept::default();
        for name in dir.names()? {
            let expected = format!("{:020}.json", kept.next);
            let recovering = name == OsStr::new("pending.new");
            ensure!(
                name == OsStr::new(&expected) || recovering,
                "incomplete or unrecognized portable capture {name:?}; originals are retained"
            );
            let record: Record =
                link_files::saved::parse_json(&dir.read(&name, LIMITS.record_max)?)
                    .context("reading portable history; originals are retained")?;
            ensure!(
                record.adapter == context.adapter && record.agent == context.agent,
                "portable history belongs to another adapter"
            );
            validate(&kept, &record.thread, &record.change)?;
            if recovering {
                dir.rename(&name, OsStr::new(&expected))?;
                dir.sync_all()?;
            }
            apply(&mut kept, record)?;
        }
        Ok(Self {
            _lock: lock,
            dir,
            context,
            kept: Mutex::new(Ok(kept)),
        })
    }

    pub fn adapter(&self) -> &str {
        &self.context.adapter
    }

    pub fn check(&self) -> anyhow::Result<()> {
        self.read(|_| ())
    }

    pub fn sessions(&self) -> anyhow::Result<Vec<Session>> {
        self.read(|kept| kept.threads.values().map(|t| t.session.clone()).collect())
    }

    pub fn session(&self, thread: &str) -> anyhow::Result<Option<Session>> {
        self.read(|kept| kept.threads.get(thread).map(|t| t.session.clone()))
    }

    pub fn import_all(&self, sessions: &[Session]) -> anyhow::Result<()> {
        let mut seen = BTreeSet::new();
        let mut incoming = Vec::new();
        for session in sessions {
            let thread = crate::serving::thread_of(&session.session_id)?;
            ensure!(
                seen.insert(thread.clone()),
                "portable import repeats a routing identity"
            );
            if let Some(old) = self.session(&thread)? {
                if old.updates.starts_with(&session.updates) {
                    continue;
                }
                ensure!(
                    session.updates.starts_with(&old.updates),
                    "portable import diverges from retained history; keep it as a separate thread"
                );
            }
            incoming.push(session.clone());
        }
        let Some(first) = incoming.first() else {
            return Ok(());
        };
        self.write(
            first.session_id.0.to_string(),
            Change::Import {
                sessions: incoming,
                pending: true,
            },
        )
    }

    /// Capture backfill without replacing a divergent branch or marking native
    /// sessions for reseeding. Both complete histories remain exportable.
    pub fn backfill(&self, mut session: Session) -> anyhow::Result<()> {
        self.read(|kept| check_sources(&kept.sources, &session.updates))??;
        let thread = session.session_id.0.to_string();
        if let Some(old) = self.session(&thread)? {
            if old.updates.starts_with(&session.updates) {
                return Ok(());
            }
            if session.updates.starts_with(&old.updates) {
                return self.append(&thread, session.updates[old.updates.len()..].to_vec(), None);
            }
            let branch = link_harness_contract::harness::continuation::native_thread(
                self.adapter(),
                &format!("native-branch:{thread}"),
            );
            if let Some(old) = self.session(&branch)? {
                if old.updates.starts_with(&session.updates) {
                    return Ok(());
                }
                if session.updates.starts_with(&old.updates) {
                    return self.append(
                        &branch,
                        session.updates[old.updates.len()..].to_vec(),
                        None,
                    );
                }
                session.session_id = link_harness_contract::harness::continuation::native_thread(
                    self.adapter(),
                    &serde_json::to_string(&session)?,
                )
                .into();
            } else {
                session.session_id = branch.into();
            }
        }
        let thread = session.session_id.0.to_string();
        if self.session(&thread)?.is_some() {
            return Ok(());
        }
        self.write(
            thread,
            Change::Import {
                sessions: vec![session],
                pending: false,
            },
        )
    }

    pub fn limitations(
        &self,
        sessions: &[Session],
    ) -> Vec<link_harness_contract::asp::methods::NotTaken> {
        sessions.iter().filter_map(|session| self.context.prepare(session,0).err().map(|error|
            link_harness_contract::asp::methods::NotTaken {kind:link_harness_contract::asp::Kind::Sessions,
                reason:format!("{} is archived completely but bounded continuation is refused: {error}",session.session_id.0)})).collect()
    }

    pub fn prepare(
        &self,
        thread: &str,
        prompt_bytes: usize,
        unbound: bool,
    ) -> anyhow::Result<Option<Prepared>> {
        let memory = serde_json::to_string(&self.context.memory)?;
        let session = self.read(|kept| match kept.threads.get(thread) {
            Some(t) if t.pending || t.memory != memory || unbound => Some(t.session.clone()),
            None if !self.context.memory.is_empty() => Some(Session {
                evidence: None,
                session_id: thread.to_string().into(),
                title: None,
                updates: Vec::new(),
                harness: None,
            }),
            _ => None,
        })?;
        session
            .map(|session| self.context.prepare(&session, prompt_bytes))
            .transpose()
    }

    pub fn admit(&self, thread: &str, prompt: &PromptRequest) -> anyhow::Result<()> {
        let said = link_connection_contract::acp::Said::of(prompt.meta.as_ref())?;
        let delivery = said
            .delivery
            .map(|id| serde_json::to_string(&id))
            .transpose()?;
        ensure!(
            !self.read(|kept| delivery.as_ref().is_some_and(|id| kept
                .threads
                .get(thread)
                .is_some_and(|t| t.deliveries.contains(id))))?,
            "this delivery is already archived; its outcome must be reconciled before replay"
        );
        let admitted = link_id::candidate()?;
        let updates = prompt
            .prompt
            .iter()
            .enumerate()
            .map(|(index, content)| {
                let mut chunk = ContentChunk::new(content.clone());
                chunk.meta = prompt.meta.clone();
                chunk.message_id = Some(format!("portable-{admitted}-{index}").into());
                SessionUpdate::UserMessageChunk(chunk)
            })
            .collect();
        self.append(thread, updates, delivery)
    }

    pub fn append(
        &self,
        thread: &str,
        updates: Vec<SessionUpdate>,
        delivery: Option<String>,
    ) -> anyhow::Result<()> {
        self.write(thread.into(), Change::Append { updates, delivery })
    }

    pub fn supplied(&self, thread: &str, context: Prepared) -> anyhow::Result<()> {
        self.write(
            thread.into(),
            Change::Supplied {
                context,
                memory: serde_json::to_string(&self.context.memory)?,
            },
        )
    }

    fn read<T>(&self, read: impl FnOnce(&Kept) -> T) -> anyhow::Result<T> {
        let kept = crate::harness::lock(&self.kept);
        Ok(read(
            kept.as_ref().map_err(|error| anyhow::anyhow!("{error}"))?,
        ))
    }

    fn write(&self, thread: String, change: Change) -> anyhow::Result<()> {
        crate::serving::thread_of(&thread.clone().into())?;
        let mut state = crate::harness::lock(&self.kept);
        let kept = state.as_mut().map_err(|error| anyhow::anyhow!("{error}"))?;
        validate(kept, &thread, &change)?;
        let record = Record {
            adapter: self.adapter().into(),
            agent: self.context.agent.clone(),
            thread,
            change,
        };
        let saved = (|| -> anyhow::Result<()> {
            let bytes = link_files::saved::print_json(&record)?;
            ensure!(
                bytes.len() as u64 <= LIMITS.record_max,
                "portable capture is too large; native originals are retained"
            );
            let name = format!("{:020}.json", kept.next);
            let mut file = self.dir.create_new(OsStr::new("pending.new"), 0o600)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            self.dir
                .rename(OsStr::new("pending.new"), OsStr::new(&name))?;
            self.dir.sync_all()?;
            apply(kept, record)
        })();
        if let Err(error) = saved {
            let reason =
                format!("portable capture is uncertain; originals are retained: {error:#}");
            *state = Err(reason.clone());
            anyhow::bail!("{reason}");
        }
        Ok(())
    }
}

fn apply(kept: &mut Kept, record: Record) -> anyhow::Result<()> {
    crate::serving::thread_of(&record.thread.clone().into())?;
    match record.change {
        Change::Import { sessions, pending } => {
            let mut seen = BTreeSet::new();
            for session in &sessions {
                crate::serving::thread_of(&session.session_id)?;
                ensure!(
                    seen.insert(session.session_id.0.to_string()),
                    "portable import repeats a thread"
                );
            }
            for session in sessions {
                index_sources(kept, &session.updates);
                let thread = thread(kept, &session.session_id.0);
                thread.session = session;
                thread.pending |= pending;
            }
        }
        Change::Append { updates, delivery } => {
            index_sources(kept, &updates);
            let thread = thread(kept, &record.thread);
            thread.session.updates.extend(updates);
            if let Some(delivery) = delivery {
                ensure!(
                    thread.deliveries.insert(delivery),
                    "duplicate portable delivery"
                );
            }
        }
        Change::Supplied { memory, .. } => {
            let thread = thread(kept, &record.thread);
            thread.pending = false;
            thread.memory = memory;
        }
    }
    kept.next = kept
        .next
        .checked_add(1)
        .context("portable capture sequence is full")?;
    Ok(())
}

fn index_sources(kept: &mut Kept, updates: &[SessionUpdate]) {
    for update in updates {
        if let Some((source, raw)) = link_harness_contract::harness::transcript::source(update) {
            kept.sources
                .insert((source.adapter, source.session, source.record), raw.clone());
        }
    }
}

fn check_sources<'a>(
    known: &BTreeMap<(String, String, String), serde_json::Value>,
    updates: impl IntoIterator<Item = &'a SessionUpdate>,
) -> anyhow::Result<()> {
    let mut seen = BTreeMap::new();
    for update in updates {
        if let Some((source, raw)) = link_harness_contract::harness::transcript::source(update) {
            let key = (source.adapter, source.session, source.record);
            if let Some(old) = known.get(&key).or_else(|| seen.get(&key).copied()) {
                ensure!(
                    old == raw,
                    "native source correlation was reused with different content; originals are retained"
                );
            }
            seen.insert(key, raw);
        }
    }
    Ok(())
}

fn validate(kept: &Kept, thread: &str, change: &Change) -> anyhow::Result<()> {
    crate::serving::thread_of(&thread.to_string().into())?;
    kept.next
        .checked_add(1)
        .context("portable capture sequence is full")?;
    match change {
        Change::Import { sessions, .. } => {
            let mut seen = BTreeSet::new();
            for session in sessions {
                let name = crate::serving::thread_of(&session.session_id)?;
                ensure!(
                    seen.insert(name.clone()),
                    "portable import repeats a routing identity"
                );
                if let Some(old) = kept.threads.get(&name) {
                    ensure!(
                        session.updates.starts_with(&old.session.updates),
                        "portable import diverges from retained history"
                    );
                }
            }
            check_sources(&kept.sources, sessions.iter().flat_map(|s| &s.updates))?;
        }
        Change::Append { updates, delivery } => {
            check_sources(&kept.sources, updates)?;
            ensure!(
                !delivery.as_ref().is_some_and(|id| kept
                    .threads
                    .get(thread)
                    .is_some_and(|t| t.deliveries.contains(id))),
                "duplicate portable delivery"
            );
        }
        Change::Supplied { context, .. } => {
            ensure!(
                context.session_id.0.as_ref() == thread
                    && context.through_updates
                        <= kept
                            .threads
                            .get(thread)
                            .map_or(0, |t| t.session.updates.len()),
                "portable continuation has an invalid archive cursor"
            );
        }
    }
    Ok(())
}

fn thread<'a>(kept: &'a mut Kept, name: &str) -> &'a mut Thread {
    kept.threads
        .entry(name.to_string())
        .or_insert_with(|| Thread {
            session: Session {
                evidence: None,
                session_id: name.to_string().into(),
                title: None,
                updates: Vec::new(),
                harness: None,
            },
            pending: false,
            memory: "[]".into(),
            deliveries: BTreeSet::new(),
        })
}
