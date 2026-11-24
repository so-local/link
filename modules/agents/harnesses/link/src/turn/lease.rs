//! A turn's lease on its conversation: who holds it, and its renewal
//! while the turn runs.

use std::sync::Arc;

use tokio::sync::watch;

use crate::agent::{Agent, Thread};
use crate::store::Store;

/// A turn's lease being renewed, until it's dropped.
pub(super) struct Renewing {
    task: tokio::task::JoinHandle<()>,
    lost: watch::Receiver<Option<String>>,
}

impl Drop for Renewing {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Renewing {
    /// Renews the lease every `every` while the turn runs, and tells it,
    /// through `cancel`, when it's asked to end; until dropped.
    #[expect(
        clippy::disallowed_methods,
        reason = "the lease's own period of renewal"
    )]
    pub(super) fn start(
        store: &Arc<dyn Store>,
        agent: &Agent,
        thread: &Thread,
        holder: &str,
        every: std::time::Duration,
        cancel: watch::Sender<bool>,
        interaction: Option<(
            link_interactions::Store,
            std::sync::Arc<link_secretive::Secret>,
        )>,
    ) -> Self {
        let (lost_tx, lost) = watch::channel(None::<String>);
        let store = store.clone();
        let (agent, thread, holder) = (agent.clone(), thread.clone(), holder.to_string());
        let task = tokio::spawn(async move {
            let mut renewed = std::time::Instant::now();
            loop {
                tokio::time::sleep(super::LOOK_EVERY).await;
                let asked = {
                    let store = store.clone();
                    let (agent, thread) = (agent.clone(), thread.clone());
                    tokio::task::spawn_blocking(move || store.interrupted(&agent, &thread))
                        .await
                        .unwrap_or(false)
                };
                let controlled = interaction.as_ref().is_some_and(|(store, token)| {
                    store.interrupted_work(token.expose()).unwrap_or(true)
                });
                if asked || controlled {
                    let _ = cancel.send(true);
                }
                if renewed.elapsed() >= every {
                    renewed = std::time::Instant::now();
                    let store = store.clone();
                    let (agent, thread, holder) = (agent.clone(), thread.clone(), holder.clone());
                    let kept = tokio::task::spawn_blocking(move || {
                        store.renew_turn(&agent, &thread, &holder)
                    })
                    .await;
                    if let Ok(Err(err)) = kept {
                        let _ = lost_tx.send(Some(format!("{err:#}")));
                        return;
                    }
                }
            }
        });
        Self { task, lost }
    }

    /// Waits until the lease is lost: why.
    pub(super) async fn lost(&self) -> anyhow::Error {
        let mut lost = self.lost.clone();
        let why = lost
            .wait_for(Option::is_some)
            .await
            .map(|why| why.clone().unwrap_or_default());
        let Ok(why) = why else {
            return std::future::pending().await;
        };
        anyhow::anyhow!("{why}")
    }
}

/// Who holds a lease: this process, and a random part, so two turns of
/// one process never take one for the other.
pub(super) fn holder() -> anyhow::Result<String> {
    let token = link_files::random_token()?;
    Ok(format!(
        "{}-{}",
        std::process::id(),
        &token[..12.min(token.len())]
    ))
}

/// The leases this process's turns hold, so a stopping harness lets go
/// of any still held ([`super::Harness::let_go`]): the next process
/// then takes the conversation at once, never waiting out the lapse
/// of a turn cut off by the stop.
#[derive(Default)]
pub(super) struct Holding {
    held: std::sync::Mutex<std::collections::HashMap<u64, (Agent, Thread, String)>>,
    next: std::sync::atomic::AtomicU64,
}

/// A turn's lease counted held until it's dropped.
pub(super) struct Holds<'a> {
    holding: &'a Holding,
    id: u64,
}

impl Drop for Holds<'_> {
    fn drop(&mut self) {
        self.holding
            .held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

impl Holding {
    /// `holder`'s lease on `thread` counted held until the guard is
    /// dropped.
    pub(super) fn hold(&self, agent: &Agent, thread: &Thread, holder: &str) -> Holds<'_> {
        let id = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, (agent.clone(), thread.clone(), holder.to_string()));
        Holds { holding: self, id }
    }

    /// Every lease held now: its agent, conversation and holder.
    pub(super) fn held(&self) -> Vec<(Agent, Thread, String)> {
        self.held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    }
}

impl super::Harness {
    /// Lets go of every lease its turns hold, for a harness stopping
    /// once its answers are given: a turn still running then, such as a
    /// summary after an answer, is cut off, and the next process to take
    /// its conversation finishes it at once rather than waiting out its
    /// lapse. How many it let go of.
    pub fn let_go(&self) -> usize {
        let held = self.holding.held();
        for (agent, thread, holder) in &held {
            let _ = super::off_runtime(|| self.store.end_turn(agent, thread, holder));
        }
        held.len()
    }
}
