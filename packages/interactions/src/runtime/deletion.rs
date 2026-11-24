use super::*;

impl Store {
    pub fn pending_deletions(&self, agent: &str) -> anyhow::Result<Vec<AddressedControl>> {
        self.pending_deletions_for(agent, "host")
    }

    pub fn pending_deletions_for(
        &self,
        agent: &str,
        source: &str,
    ) -> anyhow::Result<Vec<AddressedControl>> {
        self.read(|s| {
            Ok(s.controls
                .batches
                .values()
                .flat_map(|b| &b.members)
                .filter(|m| {
                    m.work.agent == agent
                        && matches!(m.operation, ControlOperation::Delete { .. })
                        && (source != "scheduler"
                            || matches!(
                                m.operation,
                                ControlOperation::Delete {
                                    scope: DeleteScope::TaskRecord
                                }
                            ))
                        && (source != "bridge"
                            || matches!(
                                m.operation,
                                ControlOperation::Delete {
                                    scope: DeleteScope::Conversation | DeleteScope::TaskRecord
                                }
                            ))
                        && !s
                            .controls
                            .runtime
                            .deletion_sources
                            .get(&m.id)
                            .is_some_and(|sources| sources.contains(source))
                        && !s.controls.receipts.iter().any(|r| {
                            r.correlation == m.id.as_str()
                                && matches!(
                                    r.stage,
                                    ReceiptStage::Applied
                                        | ReceiptStage::Unsupported
                                        | ReceiptStage::Denied
                                )
                        })
                })
                .cloned()
                .collect())
        })
    }

    /// The host reports only after its scoped source deletion completes under the work lease.
    pub fn deleted_work(&self, member: &AddressedControl) -> anyhow::Result<()> {
        self.deletion_applied(member, "host")
    }

    pub fn deletion_applied(&self, member: &AddressedControl, source: &str) -> anyhow::Result<()> {
        self.change(|s| {
            ensure!(
                matches!(source, "host" | "scheduler" | "bridge"),
                "unknown deletion source"
            );
            ensure!(
                s.controls
                    .batches
                    .values()
                    .any(|b| b.members.contains(member)),
                "unknown deletion member"
            );
            let ControlOperation::Delete { scope } = &member.operation else {
                anyhow::bail!("this is not a deletion")
            };
            ensure!(
                source != "scheduler" || matches!(scope, DeleteScope::TaskRecord),
                "scheduler does not own this deletion scope"
            );
            ensure!(
                source != "bridge"
                    || matches!(scope, DeleteScope::Conversation | DeleteScope::TaskRecord),
                "bridge does not own this deletion scope"
            );
            if s.controls
                .receipts
                .iter()
                .any(|r| r.correlation == member.id.as_str() && r.stage == ReceiptStage::Applied)
            {
                return Ok(());
            }
            let sources = s
                .controls
                .runtime
                .deletion_sources
                .entry(member.id.clone())
                .or_default();
            sources.insert(source.into());
            if !sources.contains("host")
                || (matches!(scope, DeleteScope::TaskRecord) && !sources.contains("scheduler"))
                || (matches!(scope, DeleteScope::Conversation | DeleteScope::TaskRecord)
                    && !sources.contains("bridge"))
            {
                return Ok(());
            }
            let b = s
                .controls
                .runtime
                .work
                .get_mut(&member.work.attempt)
                .context("unknown deletion work")?;
            ensure!(
                !b.active
                    && b.state.in_flight_effects.is_empty()
                    && b.state.lifecycle == Lifecycle::Deleting,
                "work was not shut down for deletion"
            );
            if matches!(scope, DeleteScope::Artifacts { .. }) {
                b.state.lifecycle = Lifecycle::Open;
            } else {
                b.state.lifecycle = Lifecycle::Deleted;
                b.state.checkpoint = None;
                b.retired = true;
            }
            b.token = std::sync::Arc::new(link_secretive::Secret::new(link_files::random_token()?));
            invalidate(s, &member.work, "selected work data was deleted", true);
            receipt(
                s,
                member.id.as_str(),
                &member.work,
                ReceiptStage::Applied,
                "host completed the explicit data-removal scope",
            );
            Ok(())
        })
    }
}
