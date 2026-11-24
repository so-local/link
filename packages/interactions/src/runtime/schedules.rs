use super::*;

impl Store {
    /// Projection only: the scheduler remains the owner of cadence, due time and firing claims.
    pub fn register_schedule(
        &self,
        agent: &str,
        id: &ScheduleId,
        revision: u64,
        owner_route: Option<String>,
    ) -> anyhow::Result<()> {
        let _fence = self.dispatch_fence()?;
        self.change(|s| {
            ensure!(revision > 0 && !id.0.is_empty(), "invalid schedule reference");
            let previous = s.controls.runtime.work.values().find(|b| !b.retired && b.state.work.agent == agent
                && matches!(&b.state.work.kind, WorkKind::Schedule {id: existing, ..} if existing == id)).cloned();
            if let Some(previous) = &previous
                && let WorkKind::Schedule {revision: old, ..} = previous.state.work.kind {
                    ensure!(revision >= old, "stale schedule projection");
                    if revision == old {return Ok(());}
            }
            let token = link_digest::sha256(id.0.as_bytes());
            let work = WorkRef {kind:WorkKind::Schedule {id:id.clone(), revision}, owner:sources::owner(), agent:agent.into(),
                session:format!("schedule-{}", &token[..12]), task:None, attempt:format!("schedule-{}-{revision}", &token[..12]),
                generation:revision, control_revision:0};
            let mut binding = previous.clone().unwrap_or(Binding {owner_route: owner_route.clone(), token:std::sync::Arc::new(link_secretive::Secret::new(link_files::random_token()?)), state:WorkState {
                work:work.clone(), lifecycle:Lifecycle::Open, pause_reasons:BTreeSet::new(), observed:Execution::Prepared,
                checkpoint:None, in_flight_effects:BTreeSet::new()}, routing_agent:agent.into(), parent_attempt:None,
                active:false, retired:false, interrupt:false, plan_revision:revision, wake:false});
            if let Some(previous) = previous {
                s.controls.runtime.work.get_mut(&previous.state.work.attempt).expect("known schedule").retired = true;
                for b in s.controls.runtime.work.values_mut() {
                    if b.parent_attempt.as_deref() == Some(&previous.state.work.attempt) {b.parent_attempt = Some(work.attempt.clone());}
                }
                let affected: Vec<_> = s.controls.runtime.work.values().filter(|b| !b.retired && b.state.work.agent == agent
                    && matches!(&b.state.work.kind, WorkKind::Occurrence {occurrence} if occurrence.schedule == *id && occurrence.revision < revision))
                    .map(|b| b.state.work.clone()).collect();
                for old in affected {
                    invalidate(s, &old, "schedule definition or harness generation changed", true);
                    let b = s.controls.runtime.work.get_mut(&old.attempt).expect("known occurrence");
                    b.interrupt = true;
                    b.state.work.control_revision += 1;
                }
            }
            binding.state.work = work.clone();
            binding.owner_route = owner_route;
            binding.plan_revision = revision;
            binding.token = std::sync::Arc::new(link_secretive::Secret::new(link_files::random_token()?));
            s.controls.runtime.work.insert(work.attempt.clone(), binding);
            s.controls.runtime.revision += 1;
            Ok(())
        })
    }

    pub fn schedule_ready(
        &self,
        agent: &str,
        occurrence: &ScheduledOccurrence,
    ) -> anyhow::Result<bool> {
        self.read(|s| Ok(schedule_allows(s, agent, occurrence, true)))
    }
}

pub(super) fn schedule_allows(
    s: &State,
    agent: &str,
    occurrence: &ScheduledOccurrence,
    first: bool,
) -> bool {
    s.controls.runtime.work.values().find(|b| !b.retired && b.state.work.agent == agent
        && matches!(&b.state.work.kind, WorkKind::Schedule {id, ..} if *id == occurrence.schedule))
        .is_some_and(|b| matches!(&b.state.work.kind, WorkKind::Schedule {revision, ..} if *revision == occurrence.revision)
            && (!first || progress(b)))
}
