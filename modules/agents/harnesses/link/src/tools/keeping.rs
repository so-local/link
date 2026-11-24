//! Sessions kept for the turns after, each ended once idle ([`Kept`]).

use super::*;

/// Each kept server's session ([`Server::kept`]: memory's), by agent and
/// server, kept between turns with when a turn last let it go: started
/// by a turn's call, and ended by [`Kept::end_idle`] once unused for
/// [`IDLE`], or [`AGE_MAX`] after it started. Turns at once share it:
/// MCP takes requests at once in a session. A handle: its clones keep
/// the same sessions, so a start a turn left going keeps its own.
#[derive(Clone, Default)]
pub struct Kept(Arc<std::sync::Mutex<HashMap<(Agent, Server), KeptSession>>>);

/// A kept session, when a turn last let it go, when it started, and how
/// long it lives.
type KeptSession = (Arc<Session>, std::time::Instant, std::time::Instant, Life);

impl Kept {
    pub(super) fn get(&self, agent: &Agent, server: &Server) -> Option<Arc<Session>> {
        let kept = self.0.lock().unwrap_or_else(|e| e.into_inner());
        kept.get(&(agent.clone(), server.clone()))
            .map(|(session, _, _, _)| session.clone())
    }

    pub(super) fn put(&self, agent: &Agent, server: &Server, session: Arc<Session>, life: Life) {
        let mut kept = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let now = std::time::Instant::now();
        kept.insert((agent.clone(), server.clone()), (session, now, now, life));
    }

    /// A turn let `session` go now: false if it isn't kept any more.
    pub(super) fn touch(&self, agent: &Agent, server: &Server, session: &Arc<Session>) -> bool {
        let mut kept = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((kept, at, _, _)) = kept.get_mut(&(agent.clone(), server.clone()))
            && Arc::ptr_eq(kept, session)
        {
            *at = std::time::Instant::now();
            return true;
        }
        false
    }

    /// Forgets `session`, which failed: the next call starts another.
    pub(super) fn forget(&self, agent: &Agent, server: &Server, session: &Arc<Session>) {
        let mut kept = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let key = (agent.clone(), server.clone());
        if kept
            .get(&key)
            .is_some_and(|(kept, _, _, _)| Arc::ptr_eq(kept, session))
        {
            kept.remove(&key);
        }
    }

    /// Ends each session no turn holds and none has used for as long as
    /// it's kept idle, and each started as long ago as it lives ([`Life`],
    /// each its own, or `instead`'s): at once if no turn holds it, else
    /// as the last turn holding it lets it go ([`Toolbox::close`]).
    pub async fn end_idle(&self, instead: Option<Life>) {
        let ending: Vec<Arc<Session>> = {
            let mut kept = self.0.lock().unwrap_or_else(|e| e.into_inner());
            let done: Vec<_> = kept
                .iter()
                .filter(|(_, (session, at, started, life))| {
                    let (idle, age) = instead.unwrap_or(*life);
                    let held = Arc::strong_count(session) > 1;
                    started.elapsed() >= age || (!held && at.elapsed() >= idle)
                })
                .map(|(key, _)| key.clone())
                .collect();
            done.iter()
                .filter_map(|key| kept.remove(key).map(|(session, _, _, _)| session))
                .filter(|session| Arc::strong_count(session) == 1)
                .collect()
        };
        futures_util::future::join_all(ending.iter().map(|session| session.end())).await;
    }

    /// How many sessions are kept (tests).
    pub fn len(&self) -> usize {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
