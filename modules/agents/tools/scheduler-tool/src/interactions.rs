//! Source checks for retained delegation. The scheduler keeps jobs; the host owns holds and lineage.
use super::*;
use link_interactions::{control::DelegationAction, runtime::ProposalState};
use serde_json::Value;

pub(crate) fn prepare(
    store: &Store,
    job: &Job,
    meta: &Value,
) -> anyhow::Result<Option<ProposalState>> {
    let Some(interactions) = &store.interactions else {
        return Ok(None);
    };
    let action = match job.kind {
        Kind::Task => DelegationAction::SpawnChild,
        Kind::Send | Kind::ChildMessage => DelegationAction::DispatchChildMessage,
        _ => return Ok(None),
    };
    let Some(token) = meta["link.local/work"].as_str() else {
        anyhow::ensure!(
            !store
                .interaction_agent
                .as_ref()
                .map(|agent| interactions.has_agent_work(agent))
                .transpose()?
                .unwrap_or(false),
            "delegation requires this harness's authenticated work binding"
        );
        return Ok(None);
    };
    if let Some(agent) = &store.interaction_agent {
        interactions.authenticate_work(
            token,
            agent,
            meta["link.local/thread"].as_str().unwrap_or_default(),
            meta["link.local/attempt"].as_str().unwrap_or_default(),
        )?;
    }
    let child = match job.kind {
        Kind::Task => Some(job.id.clone()),
        Kind::ChildMessage => job.thread.strip_prefix("task-").map(str::to_owned),
        _ => None,
    };
    Ok(Some(
        interactions
            .propose_delegation(
                token,
                meta["link.local/control_revision"]
                    .as_u64()
                    .context("delegation has no control revision")?,
                &job.id,
                action,
                child,
                &payload(job)?,
            )?
            .state,
    ))
}

pub(crate) fn ready(store: &Store, job: &Job) -> anyhow::Result<bool> {
    let Some(interactions) = &store.interactions else {
        return Ok(true);
    };
    // The source's job claim and the dispatch record must agree before a driver receives work.
    let _fence = interactions.dispatch_fence()?;
    if let (Some(agent), Some(planned)) = (&store.interaction_agent, &job.portable)
        && !interactions.schedule_ready(agent, &planned.occurrence)?
    {
        return Ok(false);
    }
    if let (Some(agent), Some(occurrence)) = (&store.interaction_agent, &job.occurrence)
        && !interactions.schedule_ready(agent, occurrence)?
    {
        return Ok(false);
    }
    interactions.dispatch_delegation(&job.id, &payload(job)?)
}

pub(crate) fn definition(
    store: &Store,
    definition: &link_agent_state_protocol::schedules::Schedule,
) -> anyhow::Result<()> {
    if let (Some(interactions), Some(agent)) = (&store.interactions, &store.interaction_agent) {
        let route = match &definition.to {
            link_agent_state_protocol::schedules::Destination::Thread(
                link_agent_state_protocol::schedules::DestinationThread { thread },
            ) => Some(thread.clone()),
            link_agent_state_protocol::schedules::Destination::Owner(
                link_agent_state_protocol::schedules::DestinationOwner { connection },
            ) => connection.as_ref().map(|c| format!("{c}:")),
        };
        interactions.register_schedule(agent, &definition.id, definition.revision, route)?;
    }
    Ok(())
}

pub(crate) fn delete_tasks(store: &Store) -> anyhow::Result<()> {
    if let (Some(interactions), Some(agent)) = (&store.interactions, &store.interaction_agent) {
        for job in store
            .jobs()
            .into_iter()
            .chain(store.firing())
            .filter(|j| j.kind == Kind::Task)
        {
            if interactions.child_result_delivered(agent, &job.thread)? {
                store.remove_retained(&job.id)?;
            }
        }
        for member in interactions.pending_deletions_for(agent, "scheduler")? {
            let Some(id) = &member.work.task else {
                continue;
            };
            // The host accepted deletion only after stopping this exact child.
            store.remove_retained(id)?;
            interactions.deletion_applied(&member, "scheduler")?;
        }
    }
    Ok(())
}

fn payload(job: &Job) -> anyhow::Result<Vec<u8>> {
    let mut job = job.clone();
    // Retry bookkeeping cannot change the reviewed task or message.
    job.tries = 0;
    job.due = job.created;
    Ok(serde_json::to_vec(&job)?)
}

#[cfg(test)]
mod tests;
