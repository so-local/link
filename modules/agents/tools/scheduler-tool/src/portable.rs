//! Projection into the existing scheduler. Definition activation is journaled
//! beside its jobs and guarded at dispatch, never inferred from missing jobs.
use crate::{Job, Kind, Store};
use anyhow::{Context, ensure};
use link_agent_state_protocol::schedules::{Destination, Schedule, ScheduledOccurrence, When};
use link_agent_state_protocol::schedules::{DestinationOwner, DestinationThread, WhenAt};
use serde::{Deserialize, Serialize};

const DEFINITIONS: &str = "definitions";
const LOCK: &str = "definitions.lock";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Planned {
    pub definition: Schedule,
    pub occurrence: ScheduledOccurrence,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Activation {
    definition: Schedule,
    ready: bool,
}

link_files::specification!(Activation, include_str!("portable.schema.toml"));

/// The next occurrence strictly after `at`; initial intervals also preserve
/// the supplied phase. An at schedule has exactly its recorded instant.
pub use link_scheduler_tool_contract::timing::next;

fn path(store: &Store, id: &str) -> std::path::PathBuf {
    store.dir().join(DEFINITIONS).join(format!("{id}.json"))
}

fn read(store: &Store, id: &str) -> anyhow::Result<Option<Activation>> {
    match std::fs::read(path(store, id)) {
        Ok(bytes) => Ok(Some(
            link_files::saved::parse_json(&bytes).context("reading schedule activation")?,
        )),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

pub(crate) fn lock(store: &Store) -> anyhow::Result<std::fs::File> {
    link_files::lock_file(&store.dir().join(LOCK))
}

/// Refuse dispatch while an interrupted projection is still pending.
pub(crate) fn ready(store: &Store, job: &Job) -> anyhow::Result<bool> {
    let Some(planned) = &job.portable else {
        return Ok(true);
    };
    Ok(read(store, &job.id)?
        .is_some_and(|a| a.ready && a.definition == planned.definition && a.definition.enabled))
}

/// Install/update spec definitions while their managed native source is
/// stopped and cleared. Repeated starts never recreate completed/cancelled jobs.
pub fn install(store: &Store, definitions: &[Schedule], at: u64) -> anyhow::Result<()> {
    let _lock = lock(store)?;
    for definition in definitions {
        link_scheduler_tool_contract::timing::check(definition)?;
    }
    let mut wanted = std::collections::BTreeSet::new();
    for definition in definitions {
        definition.check()?;
        crate::interactions::definition(store, definition)?;
        let id = link_digest::sha256(definition.id.0.as_bytes())[..12].to_string();
        ensure!(
            wanted.insert(id.clone()),
            "duplicate schedule identity or job hash collision"
        );
        ensure!(
            store.get(&id).is_none_or(|j| j.portable.is_some())
                && store
                    .firing()
                    .iter()
                    .all(|j| j.id != id || j.portable.is_some()),
            "schedule identity collides with a tool-created job"
        );
        let previous = read(store, &id)?;
        if let Some(previous) = &previous {
            ensure!(
                previous.definition.id == definition.id,
                "schedule job identity collision"
            );
            if previous.definition == *definition && previous.ready {
                continue;
            }
            ensure!(
                previous.definition == *definition
                    || definition.revision > previous.definition.revision,
                "changed schedule needs a newer revision"
            );
        }
        ensure!(
            !store.firing().iter().any(|j| j.id == id),
            "schedule is firing; stop its runner before changing its definition"
        );
        let activation = Activation {
            definition: definition.clone(),
            ready: false,
        };
        link_files::write_private(
            &path(store, &id),
            &link_files::saved::print_json(&activation)?,
        )?;
        store.cancel(&id)?;
        if definition.enabled {
            let due = match &definition.when {
                When::At(WhenAt { at }) => Some(*at),
                _ => next(&definition.when, at)?,
            }
            .context("schedule has no representable next occurrence")?;
            let (thread, answer) = match &definition.to {
                Destination::Thread(DestinationThread { thread }) => {
                    (thread.clone(), link_connection_contract::AnswerTo::Chat)
                }
                Destination::Owner(DestinationOwner { connection }) => (
                    format!("schedule-{id}"),
                    link_connection_contract::AnswerTo::Connection(
                        connection.clone().unwrap_or_else(|| "cli".into()),
                    ),
                ),
            };
            store.put(&Job {
                occurrence: None,
                id: id.clone(),
                kind: Kind::Schedule,
                thread,
                text: definition.message.clone(),
                answer,
                due,
                cron: None,
                hops: 1,
                created: at,
                tries: 0,
                tools: None,
                // The owner wrote the agent's spec.
                standing: link_connection_contract::access::Standing::Owner,
                model: None,
                to: None,
                portable: Some(Planned {
                    definition: definition.clone(),
                    occurrence: ScheduledOccurrence {
                        schedule: definition.id.clone(),
                        revision: definition.revision,
                        scheduled_at: due,
                    },
                }),
                parent: None,
                after: Vec::new(),
                description: None,
                outcome: None,
            })?;
        }
        link_files::write_private(
            &path(store, &id),
            &link_files::saved::print_json(&Activation {
                ready: true,
                ..activation
            })?,
        )?;
    }
    // Only jobs installed from a spec are removed. Tool-created jobs survive.
    for job in store.jobs().into_iter().chain(store.firing()) {
        if job.portable.is_some() && !wanted.contains(&job.id) {
            store.cancel(&job.id)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
