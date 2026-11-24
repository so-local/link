//! Agent state over ASP (`_asp/state/describe`, `export` and `import`),
//! answered for the harness behind the hub: by the harness itself when
//! its `initialize` advertises agent state, else by its adapter's
//! [`Store`], which reads and writes the harness's own files, and by the
//! harness's own `session/list` and `session/load` for the sessions a
//! store doesn't give. Either way
//! a session is named by its thread, as the hub names it to a bridge: an
//! export renames each of the harness's sessions to its thread, and an
//! import keeps each session that came in as a thread of its own.

use std::sync::Arc;

use agent_client_protocol::schema::v1::{ListSessionsRequest, LoadSessionRequest, SessionId};
use link_harness_contract::asp::forms::Session;
use link_harness_contract::asp::methods::{
    Describe, Described, Export, Exported, Import, Imported, NotTaken,
};
use link_harness_contract::asp::{AgentState, Kind, State};

use crate::Hub;
use crate::activity::Capture;
use crate::harness::{Route, To, Up};

/// The harness's own store of what the agent is and said, as its adapter
/// reads and writes it, for a harness whose ACP speaks no agent state.
/// Its sessions are named by the harness's own ids.
pub trait Store: Send + Sync {
    /// The kinds it gives and takes.
    fn kinds(&self) -> AgentState;
    /// The `kinds` asked for, of those it gives.
    fn export(&self, kinds: &[Kind]) -> anyhow::Result<State>;
    /// `state` written into the harness's store: each session's new id
    /// by its id in `state`, and each kind it couldn't take, with why.
    fn import(&self, state: State) -> anyhow::Result<Imported>;
}

/// Who answers for the harness's state: the harness, or its adapter's
/// store, if any, with its sessions replayed when the harness lists and
/// loads them and the store doesn't give them.
enum Source {
    Harness,
    Kept {
        store: Option<Arc<dyn Store>>,
        replays: bool,
    },
}

impl Hub {
    fn source(&self, up: &Up) -> Source {
        let caps = &up.initialized.agent_capabilities;
        if AgentState::of(caps).is_some() {
            return Source::Harness;
        }
        Source::Kept {
            store: self.state.clone(),
            replays: caps.load_session && caps.session_capabilities.list.is_some(),
        }
    }

    /// What it advertises of agent state: the harness's own, else its
    /// store's kinds and the sessions it replays.
    pub(crate) fn advertised(&self, up: &Up) -> Option<AgentState> {
        let mut kinds = match self.source(up) {
            Source::Harness => AgentState::of(&up.initialized.agent_capabilities),
            Source::Kept { store, replays } => {
                let mut kinds = store.map(|store| store.kinds()).unwrap_or_default();
                if replays && !kinds.export.contains(&Kind::Sessions) {
                    kinds.export.insert(0, Kind::Sessions);
                }
                (!kinds.export.is_empty() || !kinds.import.is_empty()).then_some(kinds)
            }
        };
        if self.continuity.is_some() {
            let kinds = kinds.get_or_insert_with(AgentState::default);
            if !kinds.export.contains(&Kind::Sessions) {
                kinds.export.push(Kind::Sessions);
            }
            if !kinds.import.contains(&Kind::Sessions) {
                kinds.import.push(Kind::Sessions);
            }
        }
        kinds
    }

    pub(crate) async fn describe(&self) -> anyhow::Result<Described> {
        if self.continuity.is_some() {
            return Ok(counted(&self.export(Kind::ALL.to_vec()).await?.state));
        }
        let capture = self.activity.capture().await?;
        let up = self.up().await?;
        if let Source::Harness = self.source(&up) {
            return Ok(up.cx.send_request(Describe {}).block_task().await?);
        }
        let kinds = self.advertised(&up).unwrap_or_default().export;
        Ok(counted(&self.kept(&up, kinds, &capture).await?))
    }

    pub(crate) async fn export(&self, kinds: Vec<Kind>) -> anyhow::Result<Exported> {
        let capture = self.activity.capture().await?;
        let up = self.up().await?;
        let sessions_requested = kinds.contains(&Kind::Sessions);
        let mut state = match self.source(&up) {
            Source::Harness => {
                up.cx
                    .send_request(Export { kinds })
                    .block_task()
                    .await?
                    .state
            }
            Source::Kept { .. } => self.kept(&up, kinds, &capture).await?,
        };
        let mut seen = std::collections::BTreeSet::new();
        for session in state.sessions.iter_mut().flatten() {
            anyhow::ensure!(
                seen.insert(session.session_id.0.to_string()),
                "native export repeats a session identity"
            );
            if let Some(thread) = self.sessions.thread_of(&session.session_id.0)? {
                session.session_id = SessionId::new(thread);
            } else if let Some(archive) = &self.continuity {
                let native = session.session_id.0.to_string();
                let thread = link_harness_contract::harness::continuation::native_thread(
                    archive.adapter(),
                    &native,
                );
                session.session_id = SessionId::new(thread);
            }
            if let Some(archive) = &self.continuity {
                archive.backfill(session.clone())?;
            }
        }
        if sessions_requested && let Some(archive) = &self.continuity {
            state.sessions = Some(archive.sessions()?);
        }
        Ok(Exported { state })
    }

    /// The `kinds` asked for, from the store and the harness's replays.
    async fn kept(&self, up: &Up, kinds: Vec<Kind>, capture: &Capture) -> anyhow::Result<State> {
        let Source::Kept { store, replays } = self.source(up) else {
            unreachable!("asked only of a harness that speaks no agent state");
        };
        let gives = store
            .as_ref()
            .map(|store| store.kinds().export)
            .unwrap_or_default();
        let replayed =
            replays && kinds.contains(&Kind::Sessions) && !gives.contains(&Kind::Sessions);
        let asked: Vec<Kind> = kinds.into_iter().filter(|k| gives.contains(k)).collect();
        let mut state = match store {
            Some(store) if !asked.is_empty() => {
                blocking(capture.clone(), move || store.export(&asked)).await?
            }
            _ => State::default(),
        };
        if replayed {
            state.sessions = Some(self.replayed(up).await?);
        }
        Ok(state)
    }

    /// Each session the harness lists, as its `session/load` replays it.
    async fn replayed(&self, up: &Up) -> anyhow::Result<Vec<Session>> {
        let mut sessions = Vec::new();
        let mut cursor = None;
        loop {
            let mut asked = ListSessionsRequest::new();
            asked.cursor = cursor.take();
            let listed = up.cx.send_request(asked).block_task().await?;
            for info in listed.sessions {
                let id = info.session_id.0.to_string();
                let (kept, mut heard) = tokio::sync::mpsc::unbounded_channel();
                let route = Route {
                    to: To::Kept(kept),
                    session: info.session_id.clone(),
                };
                let routing = up.replay_route(&id, route);
                let asked = LoadSessionRequest::new(info.session_id.clone(), self.cwd.clone());
                let loaded = up.cx.send_request(asked).block_task().await;
                drop(routing);
                loaded?;
                up.open(&id);
                let mut updates = Vec::new();
                while let Ok(update) = heard.try_recv() {
                    updates.push(update);
                }
                sessions.push(Session {
                    evidence: None,
                    harness: None,
                    session_id: info.session_id,
                    title: info.title,
                    updates,
                });
            }
            match listed.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        Ok(sessions)
    }

    pub(crate) async fn import(&self, mut state: State) -> anyhow::Result<Imported> {
        let capture = self.activity.capture().await?;
        let up = self.up().await?;
        let native_incoming = state.sessions.clone();
        let omissions = std::mem::take(&mut state.omissions);
        let takes_sessions = match self.source(&up) {
            Source::Harness => AgentState::of(&up.initialized.agent_capabilities)
                .is_some_and(|s| s.import.contains(&Kind::Sessions)),
            Source::Kept { store, .. } => {
                store.is_some_and(|s| s.kinds().import.contains(&Kind::Sessions))
            }
        };
        let portable = if self.continuity.is_some() && !takes_sessions {
            state.sessions.take()
        } else {
            None
        };
        // Validate and retain the portable input before touching native state.
        if let (Some(archive), Some(sessions)) = (&self.continuity, &portable) {
            archive.import_all(sessions)?;
        }
        let mut imported = match self.source(&up) {
            Source::Harness => up.cx.send_request(Import { state }).block_task().await?,
            Source::Kept {
                store: Some(store), ..
            } => blocking(capture.clone(), move || store.import(state)).await?,
            Source::Kept { store: None, .. } => Imported {
                not_taken: state
                    .kinds()
                    .into_iter()
                    .map(|kind| NotTaken {
                        kind,
                        reason: "this harness takes in no state from elsewhere".into(),
                    })
                    .collect(),
                ..Imported::default()
            },
        };
        // Each session a thread: its old name where it's a thread's, the
        // thread's conversation as it stands now, so its chat follows it
        // there, over a session the harness had for it before; a fresh
        // one where it isn't.
        let mut bindings = std::collections::BTreeMap::new();
        for (old, new) in imported.sessions.iter_mut() {
            let named = match crate::serving::thread_of(old) {
                Ok(thread) => thread,
                Err(_) => crate::serving::fresh()?,
            };
            bindings.insert(named.clone(), new.0.to_string());
            *new = SessionId::new(named);
        }
        if !bindings.is_empty() {
            self.sessions.keep_all(bindings)?;
            up.forget_sessions();
        }
        if portable.is_none()
            && let (Some(archive), Some(sessions)) = (&self.continuity, native_incoming)
        {
            for mut session in sessions {
                if let Some(thread) = imported.sessions.get(&session.session_id) {
                    session.session_id = thread.clone();
                    archive.backfill(session)?;
                }
            }
        }
        if let Some(sessions) = portable {
            if let Some(archive) = &self.continuity {
                imported.not_taken.extend(archive.limitations(&sessions));
            }
            for session in sessions {
                imported
                    .sessions
                    .insert(session.session_id.clone(), session.session_id);
            }
            imported.taken.push(Kind::Sessions);
            imported.not_taken.push(NotTaken {kind:Kind::Sessions, reason:
                "native session state cannot be imported; complete portable history is retained and bounded context will accompany the next turn; historical tools will not execute".into()});
        }
        imported.not_taken.extend(omissions);
        Ok(imported)
    }
}

/// How many of each kind `state` holds: one for instructions it has.
pub(crate) fn counted(state: &State) -> Described {
    let given = [
        (Kind::Sessions, state.sessions.as_ref().map(Vec::len)),
        (Kind::Instructions, state.instructions.as_ref().map(|_| 1)),
        (Kind::Skills, state.skills.as_ref().map(Vec::len)),
        (Kind::Tools, state.tools.as_ref().map(Vec::len)),
        (Kind::Memory, state.memory.as_ref().map(Vec::len)),
        (Kind::Settings, state.settings.as_ref().map(|s| s.len())),
        (
            Kind::Schedules,
            state.schedules.as_ref().map(|s| s.carried.len()),
        ),
    ];
    Described {
        kinds: given
            .into_iter()
            .filter_map(|(kind, n)| Some((kind, n?)))
            .collect(),
    }
}

async fn blocking<T: Send + 'static>(
    capture: Capture,
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    tokio::task::spawn_blocking(move || {
        let _capture = capture;
        work()
    })
    .await?
}
