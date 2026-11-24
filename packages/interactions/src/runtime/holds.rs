use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalState {
    Held,
    NeedsRevalidation,
    Ready,
    Dispatched,
    Abandoned,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegationProposal {
    pub id: String,
    pub coordinator: WorkRef,
    pub action: DelegationAction,
    pub child: Option<String>,
    pub plan_revision: u64,
    pub payload_digest: String,
    pub state: ProposalState,
}

impl Store {
    /// Keep the actual job in its source store. This record binds its exact bytes and plan.
    pub fn propose_delegation(
        &self,
        token: &str,
        revision: u64,
        id: &str,
        action: DelegationAction,
        child: Option<String>,
        payload: &[u8],
    ) -> anyhow::Result<DelegationProposal> {
        let _fence = self.dispatch_fence()?;
        self.change(|s| {
            let b = bound(s, token)?.clone();
            ensure!(
                b.active && progress(&b) && b.state.work.control_revision == revision,
                "coordinator work is fenced or changed"
            );
            let work = b.state.work.clone();
            let plan_revision = b.plan_revision;
            let digest = link_digest::sha256(payload);
            if let Some(previous) = s.controls.runtime.proposals.get(id) {
                ensure!(
                    previous.coordinator == work
                        && previous.payload_digest == digest
                        && previous.action == action
                        && previous.child == child,
                    "delegation proposal changed"
                );
                return Ok(previous.clone());
            }
            if action == DelegationAction::SpawnChild {
                children::prepare(
                    s,
                    &b,
                    child.as_deref().context("spawn has no child identity")?,
                )?;
            }
            if action == DelegationAction::DispatchChildMessage && child.is_some() {
                ensure!(s.controls.runtime.work.values().any(|c| !c.retired
                    && c.parent_attempt.as_deref() == Some(&work.attempt)
                    && matches!(&c.state.work.kind, WorkKind::Child {id} if Some(id) == child.as_ref())), "foreign child message");
            }
            let holds: Vec<_> = s.controls.runtime.holds.values().cloned().collect();
            let held =
                !link_controls::blocked_by(&holds, &work, child.as_deref(), &action).is_empty();
            let proposal = DelegationProposal {
                id: id.into(),
                coordinator: work,
                action,
                child,
                plan_revision,
                payload_digest: digest,
                state: if held {
                    ProposalState::Held
                } else {
                    ProposalState::Ready
                },
            };
            s.controls
                .runtime
                .proposals
                .insert(id.into(), proposal.clone());
            Ok(proposal)
        })
    }

    pub fn delegation_proposals(&self, token: &str) -> anyhow::Result<Vec<DelegationProposal>> {
        self.change(|s| {
            let b = bound(s, token)?;
            Ok(s.controls
                .runtime
                .proposals
                .values()
                .filter(|p| same_work(&p.coordinator, &b.state.work))
                .cloned()
                .collect())
        })
    }

    /// A model's revalidation never releases an owner hold. Source checks still run at dispatch.
    pub fn revalidate_delegation(
        &self,
        token: &str,
        revision: u64,
        id: &str,
        keep: bool,
    ) -> anyhow::Result<DelegationProposal> {
        let _fence = self.dispatch_fence()?;
        self.change(|s| {
            let b = bound(s, token)?;
            ensure!(
                b.active && progress(b) && b.state.work.control_revision == revision,
                "coordinator work is fenced or changed"
            );
            let mut p = s
                .controls
                .runtime
                .proposals
                .get(id)
                .context("unknown proposal")?
                .clone();
            ensure!(same_work(&p.coordinator, &b.state.work), "foreign proposal");
            ensure!(
                matches!(
                    p.state,
                    ProposalState::Held | ProposalState::NeedsRevalidation | ProposalState::Ready
                ) || children::unstarted(s, &p),
                "proposal already settled"
            );
            let holds: Vec<_> = s.controls.runtime.holds.values().cloned().collect();
            ensure!(
                !keep
                    || link_controls::blocked_by(
                        &holds,
                        &b.state.work,
                        p.child.as_deref(),
                        &p.action
                    )
                    .is_empty(),
                "owner hold is still active"
            );
            p.coordinator = b.state.work.clone();
            p.plan_revision = b.plan_revision;
            p.state = if keep {
                ProposalState::Ready
            } else {
                ProposalState::Abandoned
            };
            if !keep
                && children::unstarted(s, &p)
                && let Some(attempt) =
                    children::prepared(s, &p).map(|b| b.state.work.attempt.clone())
            {
                let child = s
                    .controls
                    .runtime
                    .work
                    .get_mut(&attempt)
                    .expect("prepared child");
                child.state.lifecycle = Lifecycle::Archived;
                child.state.observed = Execution::Stopped;
                child.wake = false;
                s.controls.runtime.revision += 1;
            }
            s.controls.runtime.proposals.insert(id.into(), p.clone());
            Ok(p)
        })
    }

    pub fn delegation_abandoned(&self, id: &str) -> anyhow::Result<bool> {
        self.change(|s| {
            Ok(s.controls
                .runtime
                .proposals
                .get(id)
                .is_some_and(|p| p.state == ProposalState::Abandoned))
        })
    }

    /// Caller holds the dispatch fence through source admission. Missing records are independent work.
    pub fn dispatch_delegation(&self, id: &str, payload: &[u8]) -> anyhow::Result<bool> {
        self.change(|s| {
            let Some(p) = s.controls.runtime.proposals.get(id).cloned() else {
                return Ok(true);
            };
            ensure!(
                p.payload_digest == link_digest::sha256(payload),
                "retained delegation payload changed"
            );
            let b = s
                .controls
                .runtime
                .work
                .values()
                .find(|b| !b.retired && same_work(&p.coordinator, &b.state.work))
                .context("coordinator work no longer exists")?;
            let owner_start = children::prepared(s, &p).is_some_and(|c| {
                p.action == DelegationAction::SpawnChild
                    && c.state.work.generation == 0
                    && c.wake
                    && progress(c)
            });
            if !progress(b) && !owner_start {
                return Ok(false);
            }
            let holds: Vec<_> = s.controls.runtime.holds.values().cloned().collect();
            if !owner_start
                && !link_controls::blocked_by(&holds, &b.state.work, p.child.as_deref(), &p.action)
                    .is_empty()
            {
                return Ok(false);
            }
            if (p.state != ProposalState::Ready
                && !(p.state == ProposalState::Dispatched && children::unstarted(s, &p))
                && !(owner_start
                    && matches!(
                        p.state,
                        ProposalState::Held | ProposalState::NeedsRevalidation
                    )))
                || p.plan_revision != b.plan_revision
            {
                return Ok(false);
            }
            if p.action == DelegationAction::SpawnChild {
                let child = children::prepared(s, &p).context("missing prepared child")?;
                if !progress(child)
                    || (!owner_start
                        && !children::allows_start(
                            s,
                            b,
                            p.child.as_deref().context("missing child")?,
                        ))
                {
                    return Ok(false);
                }
            }
            if p.action == DelegationAction::DispatchChildMessage
                && p.child.is_some()
                && !children::prepared(s, &p)
                    .is_some_and(|c| progress(c) && c.state.work.generation > 0)
            {
                return Ok(false);
            }
            s.controls
                .runtime
                .proposals
                .get_mut(id)
                .expect("known proposal")
                .state = ProposalState::Dispatched;
            Ok(true)
        })
    }
}

fn same_work(a: &WorkRef, b: &WorkRef) -> bool {
    a.owner == b.owner && a.agent == b.agent && a.session == b.session
}
