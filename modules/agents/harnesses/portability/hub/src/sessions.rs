//! Which of the harness's sessions is each of Link's threads, kept in a
//! file so a thread goes on where it was after a restart.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

type Bindings = BTreeMap<String, String>;

/// The file: each thread's session.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Kept {
    pub(crate) threads: Bindings,
}

link_files::specification!(Kept, include_str!("sessions.schema.toml"));

/// The hub's sessions, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Kept>()
}

pub(crate) struct Sessions {
    file: PathBuf,
    kept: Mutex<Result<Bindings, String>>,
}

impl Sessions {
    /// The map kept at `file`. Only a missing file means no saved sessions.
    /// Unreadable or invalid metadata refuses every operation until repaired.
    pub fn at(file: PathBuf) -> Self {
        let kept = (|| -> anyhow::Result<Bindings> {
            let bytes = match std::fs::read(&file) {
                Ok(bytes) => bytes,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(Bindings::new());
                }
                Err(err) => return Err(err.into()),
            };
            let kept = link_files::saved::parse_json::<Kept>(&bytes)?.threads;
            check(&kept)?;
            Ok(kept)
        })()
        .map_err(|err| {
            format!(
                "reading saved session bindings at {}: {err:#}",
                file.display()
            )
        });
        Sessions {
            file,
            kept: Mutex::new(kept),
        }
    }

    /// `thread`'s session, if it has one.
    pub fn of(&self, thread: &str) -> anyhow::Result<Option<String>> {
        self.read(|kept| kept.get(thread).cloned())
    }

    /// Refuse a runtime start while saved routing cannot be read safely.
    pub fn check(&self) -> anyhow::Result<()> {
        self.read(|_| ())
    }

    /// The thread whose session is `session`, if one's is.
    pub fn thread_of(&self, session: &str) -> anyhow::Result<Option<String>> {
        self.read(|kept| {
            kept.iter()
                .find_map(|(thread, kept)| (kept == session).then(|| thread.clone()))
        })
    }

    /// Every thread kept.
    pub fn threads(&self) -> anyhow::Result<Vec<String>> {
        self.read(|kept| kept.keys().cloned().collect())
    }

    /// Keeps `session` as `thread`'s, written before it returns: under
    /// the map's lock, so two threads kept at once don't share the file
    /// written aside.
    pub fn keep(&self, thread: &str, session: &str) -> anyhow::Result<()> {
        self.keep_all(Bindings::from([(thread.to_string(), session.to_string())]))
    }

    /// Commit an imported group together. Validation sees the final map,
    /// including swaps, and a failed write publishes none of the changes.
    pub fn keep_all(&self, incoming: Bindings) -> anyhow::Result<()> {
        let mut state = self.lock();
        let kept = state.as_mut().map_err(|err| anyhow::anyhow!("{err}"))?;
        let mut next = kept.clone();
        next.extend(incoming);
        check(&next)?;
        let bytes = link_files::saved::print_json(&Kept {
            threads: next.clone(),
        })?;
        if let Some(folder) = self.file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let aside = self.file.with_extension("new");
        link_files::write_private(&aside, &bytes)?;
        std::fs::rename(&aside, &self.file)?;
        *kept = next;
        Ok(())
    }

    fn read<T>(&self, read: impl FnOnce(&Bindings) -> T) -> anyhow::Result<T> {
        let state = self.lock();
        state
            .as_ref()
            .map(read)
            .map_err(|err| anyhow::anyhow!("{err}"))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Result<Bindings, String>> {
        self.kept.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn check(kept: &Bindings) -> anyhow::Result<()> {
    let mut native = std::collections::BTreeSet::new();
    for (thread, session) in kept {
        crate::serving::thread_of(&agent_client_protocol::schema::v1::SessionId::new(
            thread.clone(),
        ))?;
        anyhow::ensure!(!session.is_empty(), "empty saved native session");
        anyhow::ensure!(
            native.insert(session),
            "native session is bound to several threads"
        );
    }
    Ok(())
}
