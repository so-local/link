//! Prepared child identities exist before dispatch; the scheduler retains their exact input.
use super::*;

pub(super) fn prepare(s: &mut State, parent: &Binding, id: &str) -> anyhow::Result<()> {
    crate::validation::text(id)?;
    let session = format!("task-{id}");
    let key = child_key(&parent.state.work.agent, &session);
    ensure!(
        !s.controls.runtime.children.contains_key(&key),
        "child identity already exists"
    );
    let attempt = link_files::random_token()?;
    let mut child = parent.clone();
    child.token = std::sync::Arc::new(link_secretive::Secret::new(link_files::random_token()?));
    child.parent_attempt = Some(parent.state.work.attempt.clone());
    child.active = false;
    child.interrupt = false;
    child.wake = false;
    child.plan_revision = 0;
    child.state = WorkState {
        work: WorkRef {
            kind: WorkKind::Child { id: id.into() },
            session,
            task: Some(id.into()),
            attempt: attempt.clone(),
            generation: 0,
            control_revision: 0,
            ..parent.state.work.clone()
        },
        lifecycle: Lifecycle::Open,
        pause_reasons: BTreeSet::new(),
        observed: Execution::Prepared,
        checkpoint: None,
        in_flight_effects: BTreeSet::new(),
    };
    s.controls
        .runtime
        .children
        .insert(key, parent.state.work.attempt.clone());
    s.controls.runtime.work.insert(attempt, child);
    s.controls.runtime.revision += 1;
    Ok(())
}

pub(super) fn prepared<'a>(s: &'a State, p: &DelegationProposal) -> Option<&'a Binding> {
    let id = p.child.as_ref()?;
    s.controls.runtime.work.values().find(|b| {
        !b.retired
            && b.state.work.agent == p.coordinator.agent
            && matches!(&b.state.work.kind, WorkKind::Child {id: child} if child == id)
    })
}

pub(super) fn unstarted(s: &State, p: &DelegationProposal) -> bool {
    p.action == DelegationAction::SpawnChild
        && prepared(s, p).is_some_and(|b| {
            !b.active && b.state.work.generation == 0 && b.state.checkpoint.is_none()
        })
}

pub(super) fn allows_start(s: &State, parent: &Binding, id: &str) -> bool {
    if !progress(parent) {
        return false;
    }
    let holds: Vec<_> = s.controls.runtime.holds.values().cloned().collect();
    [DelegationAction::SpawnChild, DelegationAction::StartChild]
        .iter()
        .all(|action| {
            link_controls::blocked_by(&holds, &parent.state.work, Some(id), action).is_empty()
        })
}

impl Store {
    /// For a host whose store no source shares (a sandboxed harness's
    /// own): agent `agent`'s subagent `id` prepared as a child of its
    /// conversation `parent`'s current work, the lineage its scheduler's
    /// delegation records in the machine's store, so the subagent's
    /// firing is admitted. Its scheduler names the parent with the
    /// firing. Done once: a child already prepared, or a retry, is left
    /// as it is. Refused when the parent conversation has no work here.
    pub fn record_child(&self, agent: &str, parent: &str, id: &str) -> anyhow::Result<()> {
        let _fence = self.dispatch_fence()?;
        self.change(|s| {
            crate::validation::text(parent)?;
            let session = format!("task-{id}");
            if s.controls
                .runtime
                .children
                .contains_key(&child_key(agent, &session))
            {
                return Ok(());
            }
            let coordinator = s
                .controls
                .runtime
                .work
                .values()
                .filter(|b| {
                    !b.retired && b.state.work.agent == agent && b.state.work.session == parent
                })
                .max_by_key(|b| b.state.work.generation)
                .cloned()
                .context("the subagent's parent conversation has no work here")?;
            prepare(s, &coordinator, id)
        })
    }

    pub fn child_message_target(
        &self,
        token: &str,
        revision: u64,
        id: &str,
    ) -> anyhow::Result<String> {
        self.change(|s| {
            let parent = bound(s, token)?;
            ensure!(
                parent.active && progress(parent) && parent.state.work.control_revision == revision,
                "coordinator changed before child message"
            );
            let child = s
                .controls
                .runtime
                .work
                .values()
                .find(|b| {
                    !b.retired
                        && b.parent_attempt.as_deref() == Some(&parent.state.work.attempt)
                        && matches!(&b.state.work.kind, WorkKind::Child {id: child} if child == id)
                })
                .context("no retained child of this coordinator has that ID")?;
            ensure!(
                child.state.lifecycle == Lifecycle::Open,
                "child is archived or deleted"
            );
            Ok(child.state.work.session.clone())
        })
    }

    pub fn retained_children(&self, token: &str) -> anyhow::Result<Vec<WorkState>> {
        self.change(|s| {
            let parent = bound(s, token)?;
            ensure!(parent.active && progress(parent), "coordinator is fenced");
            Ok(s.controls
                .runtime
                .work
                .values()
                .filter(|b| {
                    !b.retired && b.parent_attempt.as_deref() == Some(&parent.state.work.attempt)
                })
                .map(|b| b.state.clone())
                .collect())
        })
    }

    /// The source may retry only while the host proves that no task was admitted.
    pub fn delegation_unstarted(&self, id: &str) -> anyhow::Result<bool> {
        self.change(|s| {
            Ok(s.controls
                .runtime
                .proposals
                .get(id)
                .is_some_and(|p| unstarted(s, p)))
        })
    }
}
